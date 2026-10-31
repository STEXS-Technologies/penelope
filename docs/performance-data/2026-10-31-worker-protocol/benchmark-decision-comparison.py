#!/usr/bin/env python3
"""Compare unchanged saga/contract benchmarks at two exact revisions."""
import argparse
from datetime import datetime, timezone
import hashlib
import io
import math
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tarfile
import tempfile

BENCHMARKS = {
    "penelope-executor": ["linear_saga", "graph_saga", "replay_saga",
                          "parallel_linear_saga", "parallel_graph_saga"],
    "penelope-ports": ["contract_throughput"],
}

PERF_EVENTS = ["task-clock", "instructions", "cycles", "context-switches", "cpu-migrations"]


def command(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cpu_list(value):
    cpus = [int(part) for part in value.split(",")]
    if not cpus or len(set(cpus)) != len(cpus):
        raise ValueError("CPU list must contain distinct comma-separated integers")
    if not set(cpus) <= os.sched_getaffinity(0):
        raise ValueError("CPU list must be inside the current affinity mask")
    return cpus


def observations(stdout):
    result = []
    for line in stdout.splitlines():
        count = re.search(r": (\d+) (?:isolated operations|operations|logs)", line)
        elapsed = re.search(r" in ([0-9.]+)(ns|µs|ms|s)(?: |$)", line)
        if count is None or elapsed is None:
            raise ValueError(f"unrecognized benchmark output: {line!r}")
        operations = int(count[1])
        nanos = float(elapsed[1]) * {"ns": 1, "µs": 1000, "ms": 1_000_000,
                                   "s": 1_000_000_000}[elapsed[2]]
        if operations < 1 or nanos <= 0:
            raise ValueError("benchmark must report positive work and elapsed time")
        result.append({"operation": line.split(":", 1)[0], "operations": operations,
                       "elapsed_ns": nanos, "ns_per_operation": nanos / operations})
    if not result:
        raise ValueError("benchmark produced no observations")
    return result


def performance_counters(stdout):
    counters = [json.loads(line) for line in stdout.splitlines() if line.strip()]
    if len(counters) != len(PERF_EVENTS) or {item["event"] for item in counters} != set(PERF_EVENTS):
        raise ValueError("perf must report each expected counter exactly once")
    for item in counters:
        value = float(item["counter-value"])
        if not math.isfinite(value) or value < 0 or not 0 < float(item["pcnt-running"]) <= 100:
            raise ValueError("perf counter must be finite, nonnegative and enabled")
    return counters


def build(root, toolchain, selection):
    binaries = {}
    for package, names in BENCHMARKS.items():
        selected = [name for name in names if name in selection]
        if not selected:
            continue
        arguments = [argument for name in selected for argument in ["--bench", name]]
        built = command("cargo", f"+{toolchain}", "bench", "-p", package,
                        *arguments, "--no-run", "--locked", "--message-format=json",
                        "--target-dir", str(root / "target"),
                        cwd=root, capture_output=True)
        for line in built.stdout.splitlines():
            entry = json.loads(line)
            name = entry.get("target", {}).get("name")
            if (entry.get("reason") == "compiler-artifact" and name in selected
                    and entry.get("executable")):
                if name in binaries:
                    raise ValueError(f"duplicate binary for {name}")
                binaries[name] = Path(entry["executable"])
    expected = set(selection)
    if set(binaries) != expected:
        raise ValueError(f"benchmark binaries do not match expected set: {binaries}")
    return {name: binaries[name] for names in BENCHMARKS.values() for name in names if name in selection}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new result directory")
    parser.add_argument("--before", required=True, help="baseline git revision")
    parser.add_argument("--after", default="HEAD", help="candidate git revision")
    parser.add_argument("--cpu", type=int, required=True, help="CPU for serial benchmarks")
    parser.add_argument("--parallel-cpus", required=True, help="comma-separated pinned CPUs")
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--toolchain", default="1.99.0")
    parser.add_argument("--benchmark", action="append", choices=[name for names in BENCHMARKS.values() for name in names], help="repeat to select binaries; default is all six")
    parser.add_argument("--perf", action="store_true", help="record Linux perf counters for each measured process")
    args = parser.parse_args()
    selection = args.benchmark or [name for names in BENCHMARKS.values() for name in names]
    try:
        parallel = cpu_list(args.parallel_cpus)
        cpu_list(str(args.cpu))
    except ValueError as error:
        parser.error(str(error))
    if args.runs < 1:
        parser.error("runs must be positive")
    repository = Path(__file__).resolve().parents[1]
    revisions = {label: command("git", "rev-parse", "--verify", "--end-of-options",
                               f"{revision}^{{commit}}", cwd=repository,
                               capture_output=True).stdout.strip()
                 for label, revision in [("before", args.before), ("after", args.after)]}
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    metadata = {
        "schema": 1, "measured_at": datetime.now(timezone.utc).isoformat(),
        "revisions": revisions, "serial_cpu": args.cpu, "parallel_cpus": parallel,
        "runs": args.runs, "warmup_processes_per_binary": 1,
        "benchmarks": selection,
        "perf": {"version": command("perf", "version", capture_output=True).stdout.strip(),
                 "events": PERF_EVENTS,
                 "scope": "GNU time wrapper and benchmark, including all child threads"} if args.perf else None,
        "runner_sha256": digest(Path(__file__)),
        "build_target_policy": "separate explicit target directory in each archived snapshot",
        "order": "alternating before/after by round; fixed benchmark order within each revision",
        "rustc": command("rustc", f"+{args.toolchain}", "-vV", capture_output=True).stdout,
        "platform": platform.platform(),
        "cpu_model": next(line.split(":", 1)[1].strip() for line in
                          Path("/proc/cpuinfo").read_text().splitlines()
                          if line.startswith("model name")),
        "build_environment": {name: os.environ.get(name) for name in
                              ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS",
                               "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_TARGET_DIR",
                               "CARGO_PROFILE_BENCH_OPT_LEVEL",
                               "CARGO_PROFILE_BENCH_LTO", "CARGO_PROFILE_BENCH_CODEGEN_UNITS")},
        "cache_policy": "one warmup; OS caches uncontrolled; no cold-cache claim",
        "resources": "GNU time process wall/user/system seconds and peak RSS KiB, including threads",
        "limitations": ["pure in-process workloads, no persistence or external execution",
                        "allocations and stack high-water marks not measured",
                        "RSS is whole-process peak, not incremental library memory",
                        "each revision uses its own locked dependencies; lock hashes disclosed"],
        "sources": {}, "locks": {}, "binaries": {},
    }
    target_parent = repository / "target"
    target_parent.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="decision-comparison-", dir=target_parent) as temporary:
        snapshots = {}
        for label, revision in revisions.items():
            snapshot = Path(temporary) / label
            snapshot.mkdir()
            archive = subprocess.run(["git", "archive", revision], cwd=repository,
                                     check=True, capture_output=True).stdout
            with tarfile.open(fileobj=io.BytesIO(archive)) as files:
                files.extractall(snapshot, filter="data")
            snapshots[label] = snapshot
            metadata["sources"][label] = {f"{package}/{name}": digest(
                snapshot / "crates" / package / "benches" / f"{name}.rs")
                for package, names in BENCHMARKS.items() for name in names}
            metadata["locks"][label] = digest(snapshot / "Cargo.lock")
        if metadata["sources"]["before"] != metadata["sources"]["after"]:
            raise ValueError("benchmark source/datasets differ between revisions")
        binaries = {label: build(snapshot, args.toolchain, selection) for label, snapshot in snapshots.items()}
        metadata["binaries"] = {label: {name: digest(binary) for name, binary in built.items()}
                                for label, built in binaries.items()}
        (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
        for index in range(args.runs + 1):
            order = ["before", "after"] if index % 2 == 0 else ["after", "before"]
            for label in order:
                for name, binary in binaries[label].items():
                    affinity = parallel if name.startswith("parallel_") else [args.cpu]
                    resource_path = Path(temporary) / "resources.json"
                    measured_command = ["/usr/bin/time", "-f",
                        '{"wall_seconds":%e,"user_seconds":%U,"system_seconds":%S,"peak_rss_kib":%M}',
                        "-o", str(resource_path), str(binary)]
                    perf_path = Path(temporary) / "perf.jsonl"
                    if args.perf:
                        measured_command = ["perf", "stat", "-j", "-e", ",".join(PERF_EVENTS),
                                            "-o", str(perf_path), "--", *measured_command]
                    measured = command("taskset", "-c", ",".join(map(str, affinity)),
                                       *measured_command, capture_output=True)
                    record = {"benchmark": name, "revision": revisions[label], "cpus": affinity,
                              "stdout": measured.stdout, "stderr": measured.stderr,
                              "resources": json.loads(resource_path.read_text()),
                              "observations": observations(measured.stdout),
                              "perf": performance_counters(perf_path.read_text()) if args.perf else None}
                    if index:
                        (output / f"run-{index}-{label}-{name}.json").write_text(
                            json.dumps(record, indent=2) + "\n")
                        print(f"{index}/{args.runs} {label} {name}", flush=True)


if __name__ == "__main__":
    main()
