#!/usr/bin/env python3
"""Record portable fixture layouts and Massif stack samples, with calibration."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys

PATHS = ("pure_checkpoint_recovery", "effect_reconciliation_recovery",
         "undersized_workspace", "checkpoint_migration", "independent_completion")


def command(*args, **kwargs):
    return subprocess.run(args, text=True, check=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def stack_samples(text):
    """Reject incomplete/wrong-profile records rather than silently reporting zero."""
    if not text.startswith("desc: ") or "--stacks=yes --heap=no" not in text.splitlines()[0]:
        raise ValueError("expected a stack-only Massif profile")
    snapshots = []
    current = None
    for line in text.splitlines():
        if line.startswith("snapshot="):
            if current is not None:
                snapshots.append(current)
            current = {"snapshot": int(line.split("=", 1)[1])}
        elif current is not None and line.startswith(("mem_heap_B=", "mem_heap_extra_B=", "mem_stacks_B=")):
            key, value = line.split("=", 1)
            if key in current:
                raise ValueError("duplicate memory counter")
            current[key] = int(value)
    if current is not None:
        snapshots.append(current)
    required = {"snapshot", "mem_heap_B", "mem_heap_extra_B", "mem_stacks_B"}
    if len(snapshots) < 2 or any(set(row) != required for row in snapshots):
        raise ValueError("missing Massif snapshots or counters")
    for index, row in enumerate(snapshots):
        if row["snapshot"] != index or min(row.values()) < 0:
            raise ValueError("invalid snapshot sequence or negative counter")
        if row["mem_heap_B"] or row["mem_heap_extra_B"]:
            raise ValueError("heap tracking unexpectedly enabled")
    peak = max(row["mem_stacks_B"] for row in snapshots)
    if peak == 0:
        raise ValueError("stack instrumentation recorded no stack")
    return {"maximum_sampled_stack_bytes": peak, "snapshots": len(snapshots)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new result directory")
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--runs", type=int, default=3)
    args = parser.parse_args()
    if args.runs < 1 or args.cpu not in os.sched_getaffinity(0):
        parser.error("runs must be positive and CPU available")
    root = Path(__file__).resolve().parents[1]
    os.chdir(root)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    toolchain = "1.99.0"
    build = command("cargo", f"+{toolchain}", "test", "-p", "penelope-core",
                    "--test", "task_allocations", "--release", "--no-default-features",
                    "--locked", "--no-run", "--message-format=json", capture_output=True)
    (output / "build.jsonl").write_text(build.stdout)
    (output / "build.stderr").write_text(build.stderr)
    binaries = [entry["executable"] for line in build.stdout.splitlines()
                if (entry := json.loads(line)).get("reason") == "compiler-artifact"
                and entry["target"]["name"] == "task_allocations" and entry.get("executable")]
    if len(binaries) != 1:
        raise ValueError("expected exactly one test executable")
    binary = Path(binaries[0])
    layout = json.loads(command(str(binary), "--layout", capture_output=True).stdout)
    if any(type(value) is not int or value < 1 for value in layout.values()):
        raise ValueError("invalid type layout")
    metadata = {
        "schema": 1, "measured_at": datetime.now(timezone.utc).isoformat(),
        "revision": command("git", "rev-parse", "HEAD", capture_output=True).stdout.strip(),
        "tracked_diff": command("git", "diff", "HEAD", capture_output=True).stdout,
        "runner_sha256": digest(Path(__file__)),
        "fixture_sha256": digest(root / "crates/penelope-core/tests/task_allocations.rs"),
        "binary_sha256": digest(binary), "cargo_lock_sha256": digest(root / "Cargo.lock"),
        "rustc": command("rustc", f"+{toolchain}", "-vV", capture_output=True).stdout,
        "valgrind": command("valgrind", "--version", capture_output=True).stdout.strip(),
        "platform": platform.platform(), "cpu": args.cpu, "runs": args.runs,
        "build_environment": {key: os.environ.get(key) for key in
                              ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "RUSTC_WRAPPER")},
        "scope": "whole single-threaded test process including startup, fixtures, assertions and printing",
        "limitation": "maximum retained Massif stack sample; not a static worst-case bound or adapter qualification",
        "layout_bytes": layout,
    }
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    records = []
    cases = [("stack_baseline", ["--stack-baseline"]),
             ("stack_positive_control", ["--stack-positive-control"])]
    cases.extend((path, ["--path", path]) for path in PATHS)
    for run in range(1, args.runs + 1):
        for name, options in cases:
            prefix = output / f"run-{run}-{name}"
            report = prefix.with_suffix(".massif")
            result = command("taskset", "-c", str(args.cpu), "valgrind", "--tool=massif",
                             "--stacks=yes", "--heap=no", "--time-unit=B", "--detailed-freq=1",
                             "--max-snapshots=1000", "--peak-inaccuracy=0.0",
                             f"--massif-out-file={report}", str(binary), *options,
                             capture_output=True)
            prefix.with_suffix(".stdout").write_text(result.stdout)
            prefix.with_suffix(".stderr").write_text(result.stderr)
            if name in PATHS:
                allocation = json.loads(result.stdout)
                if allocation != {"path": name, "allocations": 0, "reallocations": 0,
                                  "deallocations": 0, "requested_bytes": 0}:
                    raise ValueError("unexpected allocation assertion output")
            records.append({"run": run, "case": name, **stack_samples(report.read_text()),
                            "massif_sha256": digest(report)})
        baseline, positive = (next(row["maximum_sampled_stack_bytes"] for row in records
                                   if row["run"] == run and row["case"] == name)
                              for name in ("stack_baseline", "stack_positive_control"))
        if positive < 65_536 or positive < baseline + 49_152:
            raise ValueError("64 KiB positive-control stack was not observed")
        print(f"Recorded run {run}", flush=True)
    (output / "measurements.json").write_text(json.dumps(records, indent=2) + "\n")
    print(output)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
