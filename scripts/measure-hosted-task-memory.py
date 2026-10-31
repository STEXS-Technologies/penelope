#!/usr/bin/env python3
"""Compare hosted supervisor allocations and sampled stack at exact revisions."""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PATHS = ("pure_checkpoint_recovery", "effect_reconciliation_recovery")


def command(*args, **kwargs):
    return subprocess.run(args, text=True, check=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fixture_source(capacity):
    source = (ROOT / "scripts/fixtures/hosted_task_memory.rs").read_text()
    marker = "const CAPACITY: usize = 4096;"
    if source.count(marker) != 1 or capacity < 8:
        raise ValueError("invalid fixture capacity or marker")
    return source.replace(marker, f"const CAPACITY: usize = {capacity};")


def validate_output(text, capacity, repetitions):
    value = json.loads(text)
    expected = {"capacity": capacity, "repetitions": repetitions, "allocations": 0,
                "reallocations": 0, "deallocations": 0, "requested_bytes": 0}
    if set(value) != set(expected) | {"supervisor_bytes", "zero_sized_ports_supervisor_bytes"}:
        raise ValueError("unexpected fixture output fields")
    if any(type(value[key]) is not int or value[key] != expected[key] for key in expected):
        raise ValueError("incorrect workload or nonzero allocations")
    if any(type(value[key]) is not int or value[key] <= 0 for key in
           ("supervisor_bytes", "zero_sized_ports_supervisor_bytes")):
        raise ValueError("invalid supervisor layout")
    return value


def native_resources(text):
    wall, user, system, rss = text.split()
    times = [float(value) for value in (wall, user, system)]
    if any(not math.isfinite(value) or value < 0 for value in times) or int(rss) < 1:
        raise ValueError("invalid process resources")
    return dict(zip(("wall_seconds", "user_seconds", "system_seconds", "peak_rss_kib"),
                    [*times, int(rss)]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--before", required=True)
    parser.add_argument("--after", default="HEAD")
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--capacity", type=int, default=4096)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--repetitions", type=int, default=10000)
    args = parser.parse_args()
    if (args.runs < 1 or args.repetitions < 1 or args.capacity < 8
            or args.cpu not in os.sched_getaffinity(0)):
        parser.error("positive runs/repetitions, capacity >= 8 and available CPU required")
    source = fixture_source(args.capacity)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "fixture.rs").write_text(source)
    spec = importlib.util.spec_from_file_location("portable_memory", ROOT / "scripts/measure-portable-task-memory.py")
    portable = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(portable)
    revisions = {label: command("git", "rev-parse", "--verify", "--end-of-options",
                               f"{revision}^{{commit}}", cwd=ROOT,
                               capture_output=True).stdout.strip()
                 for label, revision in (("before", args.before), ("after", args.after))}
    metadata = {"schema": 1, "measured_at": datetime.now(timezone.utc).isoformat(),
                "revisions": revisions, "cpu": args.cpu, "capacity": args.capacity,
                "runs": args.runs, "repetitions": args.repetitions,
                "fixture_sha256": digest(output / "fixture.rs"), "runner_sha256": digest(Path(__file__)),
                "rustc": command("rustc", "+1.99.0", "-vV", capture_output=True).stdout,
                "valgrind": command("valgrind", "--version", capture_output=True).stdout.strip(),
                "platform": platform.platform(), "snapshots": {},
                "build_environment": {key: os.environ.get(key) for key in
                    ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "CARGO_PROFILE_RELEASE_LTO")},
                "scope": "fixed in-memory ports, complete lifecycle including recovery; not SQLite/process adapters",
                "limitations": ["Massif maximum retained sample, not static worst-case bound",
                    "whole-process stack and RSS include fixtures/startup; not incremental supervisor cost",
                    "one native warmup per revision/path; OS caches and CPU frequency uncontrolled",
                    "native timing excludes compilation; no cold-cache claim",
                    "separate locked builds; no dependency version substitution"]}
    records = []
    target = ROOT / "target"
    target.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="hosted-memory-", dir=target) as temporary:
        binaries = {}
        for label, revision in revisions.items():
            snapshot = Path(temporary) / label
            snapshot.mkdir()
            archive = Path(temporary) / f"{label}.tar"
            command("git", "archive", revision, "--format=tar", f"--output={archive}", cwd=ROOT)
            with tarfile.open(archive) as stream:
                stream.extractall(snapshot, filter="data")
            fixture = snapshot / "crates/penelope-executor/examples/hosted_task_memory.rs"
            fixture.write_text(source)
            build_arguments = ("cargo", "+1.99.0", "build", "-p", "penelope-executor", "--example",
                            "hosted_task_memory", "--release", "--locked", "--message-format=json",
                            "--target-dir", str(snapshot / "target"))
            build = subprocess.run(build_arguments, cwd=snapshot, capture_output=True, text=True)
            (output / f"{label}-build.jsonl").write_text(build.stdout)
            (output / f"{label}-build.stderr").write_text(build.stderr)
            build.check_returncode()
            found = [entry["executable"] for line in build.stdout.splitlines()
                     if (entry := json.loads(line)).get("reason") == "compiler-artifact"
                     and entry["target"]["name"] == "hosted_task_memory" and entry.get("executable")]
            if len(found) != 1:
                raise ValueError("expected exactly one hosted example executable")
            binaries[label] = Path(found[0])
            metadata["snapshots"][label] = {"lock_sha256": digest(snapshot / "Cargo.lock"),
                "binary_sha256": digest(binaries[label]), "fixture_sha256": digest(fixture)}
        (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
        for label, binary in binaries.items():
            for path in PATHS:
                warmed = command("taskset", "-c", str(args.cpu), str(binary), path,
                                 str(args.repetitions), capture_output=True)
                validate_output(warmed.stdout, args.capacity, args.repetitions)
                (output / f"{label}-{path}-warmup.stdout").write_text(warmed.stdout)
        for run in range(1, args.runs + 1):
            for label in (list(binaries) if run % 2 else list(reversed(binaries))):
                binary = binaries[label]
                for path in PATHS:
                    prefix = output / f"run-{run}-{label}-{path}-native"
                    timing = prefix.with_suffix(".time")
                    measured = command("/usr/bin/time", "-f", "%e %U %S %M", "-o", str(timing),
                        "taskset", "-c", str(args.cpu), str(binary), path, str(args.repetitions),
                        capture_output=True)
                    prefix.with_suffix(".stdout").write_text(measured.stdout)
                    prefix.with_suffix(".stderr").write_text(measured.stderr)
                    records.append({"run": run, "revision": label, "case": path, "mode": "native",
                        **validate_output(measured.stdout, args.capacity, args.repetitions),
                        **native_resources(timing.read_text())})
                stack = {}
                for path in ("stack_baseline", "stack_positive_control", *PATHS):
                    prefix = output / f"run-{run}-{label}-{path}-stack"
                    report = prefix.with_suffix(".massif")
                    measured = command("taskset", "-c", str(args.cpu), "valgrind", "--tool=massif",
                        "--stacks=yes", "--heap=no", "--time-unit=B", "--detailed-freq=1",
                        "--max-snapshots=1000", "--peak-inaccuracy=0.0", f"--massif-out-file={report}",
                        str(binary), path, "1", capture_output=True)
                    prefix.with_suffix(".stdout").write_text(measured.stdout)
                    prefix.with_suffix(".stderr").write_text(measured.stderr)
                    if path in PATHS:
                        validate_output(measured.stdout, args.capacity, 1)
                    samples = portable.stack_samples(report.read_text())
                    stack[path] = samples["maximum_sampled_stack_bytes"]
                    records.append({"run": run, "revision": label, "case": path, "mode": "stack",
                                    **samples, "massif_sha256": digest(report)})
                if (stack["stack_positive_control"] < 65536
                        or stack["stack_positive_control"] < stack["stack_baseline"] + 49152):
                    raise ValueError("64 KiB positive control not observed")
            print(f"Recorded paired run {run}", flush=True)
        (output / "measurements.json").write_text(json.dumps(records, indent=2) + "\n")


if __name__ == "__main__":
    main()
