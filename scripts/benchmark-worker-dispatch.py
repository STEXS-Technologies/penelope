#!/usr/bin/env python3
"""Record pinned Linux worker dispatch/host-checkpoint-ACK measurements."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

FIELDS = ("setup_ns", "intent_commit_ns", "dispatch_start_return_ns",
          "dispatch_to_host_checkpoint_ack_ns", "dispatch_to_durable_completion_and_fence_ns")
PROFILES = ("fresh_runtime_and_store", "reused_runtime_and_store")


def command(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_report(report, samples):
    if (report["schema"] != 1 or report["journal_mode"] != "WAL" or report["synchronous"] != "FULL"
            or report["worker_input"] != [5, 1, 2, 3] or report["worker_output_u64"] != 6):
        raise ValueError("unexpected worker/storage profile")
    code = report["worker_code_sha256"]
    if len(code) != 32 or any(type(value) is not int or not 0 <= value <= 255 for value in code):
        raise ValueError("invalid worker code digest")
    if [profile["profile"] for profile in report["profiles"]] != list(PROFILES):
        raise ValueError("unexpected profile selection")
    for profile in report["profiles"]:
        reused = profile["profile"] == PROFILES[1]
        rows = profile["samples"]
        if len(rows) != samples + int(reused):
            raise ValueError("wrong sample count")
        for index, row in enumerate(rows):
            if row["index"] != index or row["warmup"] is not (reused and index == 0):
                raise ValueError("invalid sample/warmup identity")
            if any(type(row[field]) is not int or row[field] < 0 for field in FIELDS):
                raise ValueError("invalid nanosecond measurement")
            if not 0 < row[FIELDS[2]] <= row[FIELDS[3]] <= row[FIELDS[4]]:
                raise ValueError("invalid dispatch/ACK/completion order")
        measured = rows[1:] if reused else rows
        if set(profile["distributions"]) != set(FIELDS):
            raise ValueError("missing distribution")
        for field in FIELDS:
            values = sorted(row[field] for row in measured)
            expected = {"samples": samples, **{
                f"p{percent}_ns": values[(samples * percent + 99) // 100 - 1]
                for percent in (50, 95, 99)}}
            if profile["distributions"][field] != expected:
                raise ValueError("distribution differs from raw samples")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new result directory")
    parser.add_argument("--data-parent", type=Path, required=True)
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--samples", type=int, default=64)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("Linux x86_64 required")
    if args.cpu not in os.sched_getaffinity(0) or args.runs < 1 or not 1 <= args.samples <= 1024:
        parser.error("available CPU, positive runs and 1..1024 samples required")
    if not args.data_parent.is_dir():
        parser.error("data-parent must exist")
    root = Path(__file__).resolve().parents[1]
    os.chdir(root)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    toolchain = "1.99.0"
    build = command("cargo", f"+{toolchain}", "bench", "-p", "penelope-process-runtime",
                    "--bench", "dispatch", "--no-run", "--locked", "--message-format=json", capture_output=True)
    (output / "build.jsonl").write_text(build.stdout)
    (output / "build.stderr").write_text(build.stderr)
    binaries = [entry["executable"] for line in build.stdout.splitlines()
                if (entry := json.loads(line)).get("reason") == "compiler-artifact"
                and entry["target"]["name"] == "dispatch" and entry.get("executable")]
    if len(binaries) != 1:
        raise ValueError("expected one benchmark executable")
    binary = Path(binaries[0])
    data_parent = args.data_parent.resolve()
    with tempfile.TemporaryDirectory(prefix="penelope-dispatch-worker-") as temporary:
        worker = Path(temporary) / "worker"
        worker_command = ["cc", "-nostdlib", "-static", "-fno-builtin", "-fno-stack-protector", "-fno-pie", "-no-pie", "-o", str(worker), str(root / "crates/penelope-process-runtime/tests/fixtures/worker.c")]
        worker_build = command(*worker_command, capture_output=True)
        (output / "worker-build.stderr").write_text(worker_build.stderr)
        metadata = {
            "schema": 1, "measured_at": datetime.now(timezone.utc).isoformat(),
            "revision": command("git", "rev-parse", "HEAD", capture_output=True).stdout.strip(),
            "tracked_diff": command("git", "diff", "HEAD", capture_output=True).stdout,
            "source_sha256": {name: digest(root / name) for name in (
                "crates/penelope-process-runtime/benches/dispatch.rs",
                "crates/penelope-process-runtime/benches/support/dispatch.rs",
                "crates/penelope-process-runtime/tests/fixtures/worker.c",
                "scripts/benchmark-worker-dispatch.py", "Cargo.lock")},
            "binary_sha256": digest(binary), "worker_sha256": digest(worker),
            "worker_build": worker_command,
            "rustc": command("rustc", f"+{toolchain}", "-Vv", capture_output=True).stdout,
            "cc": command("cc", "--version", capture_output=True).stdout,
            "platform": platform.platform(), "cpu": args.cpu,
            "cpu_model": next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")), "runs": args.runs, "samples_per_profile": args.samples,
            "filesystem": json.loads(command("findmnt", "-J", "-T", str(data_parent), capture_output=True).stdout),
            "build_environment": {key: os.environ.get(key) for key in
                                  ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "RUSTC_WRAPPER", "CC")},
            "cache_policy": "OS cache uncontrolled; fresh stores/runtimes and warmed reuse, no cold-cache claim",
            "durability": "SQLite WAL/FULL plus persistent generation/manifest fsync; no fsync bypass",
            "rss_scope": "GNU time Linux wait4 maximum RSS in KiB; worker compilation excluded; not sum of simultaneously resident parent and worker",
            "ack_endpoint": "host checkpoint ACK write completed, not independently acknowledged worker receipt",
            "allocations": "not measured", "stack": "not measured", "baseline": "new optional adapter; no v0.2 runtime exists",
        }
        (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
        code = None
        for run in range(1, args.runs + 1):
            prefix = output / f"run-{run}"
            with prefix.with_suffix(".json").open("w") as report:
                command("/usr/bin/time", "-f", '{"peak_rss_kib":%M,"wall_seconds":%e}',
                        "-o", str(prefix.with_suffix(".resources.json")), "taskset", "-c", str(args.cpu), str(binary), stdout=report,
                        env={**os.environ, "PENELOPE_BENCH_DIR": str(data_parent), "PENELOPE_DISPATCH_SAMPLES": str(args.samples), "PENELOPE_DISPATCH_WORKER": str(worker)})
            result = json.loads(prefix.with_suffix(".json").read_text())
            validate_report(result, args.samples)
            if bytes(result["worker_code_sha256"]).hex() != digest(worker):
                raise ValueError("benchmark did not execute the pinned worker bytes")
            if code is not None and result["worker_code_sha256"] != code:
                raise ValueError("worker build changed between runs")
            code = result["worker_code_sha256"]
            print(f"Recorded run {run}", flush=True)
        print(output)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
