#!/usr/bin/env python3
"""Build once, then record pinned-CPU SQLite benchmark runs and host evidence."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys


def command(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new result directory")
    parser.add_argument("--data-parent", type=Path, required=True,
                        help="existing directory on the filesystem to measure")
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--parallel-cpus",
                        help="comma-separated CPU set for threaded workloads; defaults to --cpu")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--benchmark", choices=["durable_store", "machine_store"],
                        default="durable_store")
    parser.add_argument("--repository", type=Path,
                        help="checkout to measure; defaults to this script’s repository")
    parser.add_argument("--toolchain", default="1.100.0")
    args = parser.parse_args()
    if args.runs < 1 or args.cpu not in os.sched_getaffinity(0):
        parser.error("runs must be positive and CPU must be in the current affinity mask")
    try:
        cpus = ([int(part) for part in args.parallel_cpus.split(",")]
                if args.parallel_cpus else [args.cpu])
    except ValueError:
        parser.error("parallel-cpus must be comma-separated integers")
    if len(set(cpus)) != len(cpus) or not set(cpus) <= os.sched_getaffinity(0):
        parser.error("parallel-cpus must be distinct CPUs inside the current affinity mask")
    if not args.data_parent.is_dir():
        parser.error("data-parent must be an existing directory")
    output = args.output.resolve()
    data_parent = args.data_parent.resolve()
    runner = Path(__file__).resolve()
    root = args.repository.resolve() if args.repository else runner.parents[1]
    if not (root / "Cargo.toml").is_file():
        parser.error("repository must contain Cargo.toml")
    os.chdir(root)
    output.mkdir(parents=True, exist_ok=False)
    toolchain = args.toolchain
    build = command("cargo", f"+{toolchain}", "bench", "-p", "penelope-sqlite",
                    "--bench", args.benchmark, "--locked", "--no-run",
                    "--message-format=json", capture_output=True)
    (output / "build.jsonl").write_text(build.stdout)
    (output / "build.stderr").write_text(build.stderr)
    binaries = [entry["executable"] for line in build.stdout.splitlines()
                if (entry := json.loads(line)).get("reason") == "compiler-artifact"
                and entry["target"]["name"] == args.benchmark and entry.get("executable")]
    if len(binaries) != 1:
        raise RuntimeError(f"expected one benchmark executable, found {binaries}")
    source = root / f"crates/penelope-sqlite/benches/{args.benchmark}.rs"
    support = (["crates/penelope-sqlite/tests/support/machines.rs"]
               if args.benchmark == "machine_store" else [])
    metadata = {
        "schema": 1,
        "benchmark": args.benchmark,
        "runner_sha256": hashlib.sha256(runner.read_bytes()).hexdigest(),
        "repository": str(root),
        "measured_at": datetime.now(timezone.utc).isoformat(),
        "build_environment": {key: os.environ.get(key) for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER")},
        "revision": command("git", "rev-parse", "HEAD", capture_output=True).stdout.strip(),
        "tracked_diff": command("git", "diff", "HEAD", capture_output=True).stdout,
        "benchmark_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "support_sha256": {name: hashlib.sha256((root / name).read_bytes()).hexdigest()
                           for name in support},
        "cargo_lock_sha256": hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest(),
        "rustc": command("rustc", f"+{toolchain}", "-vV", capture_output=True).stdout,
        "platform": platform.platform(), "cpu": args.cpu, "cpus": cpus,
        "cpu_model": next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")),
        "filesystem": json.loads(command("findmnt", "-J", "-T", str(data_parent), capture_output=True).stdout),
        "data_parent": str(data_parent), "runs": args.runs,
        "cold_cache": False, "cache_policy": "OS cache uncontrolled; no global cache eviction",
        "sync_policy": "SQLite WAL with synchronous=FULL; no fsync bypass",
        "peak_rss_scope": "entire benchmark process and its database-worker thread; KiB",
        "allocations": "not measured", "supervisor_stack": "not measured",
        "external_worker_execution": "not measured",
    }
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    for index in range(args.runs):
        prefix = output / f"run-{index + 1}"
        with prefix.with_suffix(".json").open("w") as report:
            command("/usr/bin/time", "-f", '{"peak_rss_kib":%M,"wall_seconds":%e}',
                    "-o", str(prefix.with_suffix(".resources.json")),
                    "taskset", "-c", ",".join(map(str, cpus)), binaries[0], stdout=report,
                    env={**os.environ, "PENELOPE_BENCH_DIR": str(data_parent)})
        results = json.loads(prefix.with_suffix(".json").read_text())
        if results["journal_mode"] != "WAL" or results["synchronous"] != "FULL":
            raise RuntimeError("unexpected storage profile")
        if args.benchmark == "durable_store":
            if [case["records"] for case in results["tasks"]] != [16, 128, 512]:
                raise RuntimeError("unexpected task dataset")
        else:
            if [case["writers"] for case in results["profiles"]] != [1, 2, 8, 32]:
                raise RuntimeError("unexpected machine writer profiles")
            if (results["measurement_mode"] != "release-101"
                    or any(case["measured_rounds"] != 101 for case in results["profiles"])):
                raise RuntimeError("machine measurements require the full release profile")
        print(f"Recorded {prefix.name}", flush=True)
    print(output)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
