#!/usr/bin/env python3
"""Run each property test for a measured duration with fresh reproducible seeds."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROPERTIES = (
    ("penelope_executor", "entry::engine::tests::retry_attempt_is_monotonic_within_the_bounded_retry_policy"),
    ("penelope_executor", "entry::engine::tests::live_transitions_and_event_replay_converge"),
    ("task_recovery", "seeded_checkpoint_crash_and_persistence_chaos"),
)


def positive(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return number


def build_tests():
    result = subprocess.run(
        ["cargo", "test", "-p", "penelope-executor", "--lib", "--test", "task_recovery",
         "--all-features", "--locked", "--no-run", "--message-format=json"],
        cwd=ROOT, text=True, capture_output=True, check=True,
    )
    executables = {}
    for line in result.stdout.splitlines():
        artifact = json.loads(line)
        if artifact["reason"] == "compiler-artifact" and artifact.get("executable"):
            executables[artifact["target"]["name"]] = artifact["executable"]
    # A nonexistent --exact filter otherwise succeeds while executing no tests.
    for target, name in PROPERTIES:
        listing = subprocess.run([executables[target], "--list"], text=True,
                                 capture_output=True, check=True)
        if f"{name}: test" not in listing.stdout.splitlines():
            raise RuntimeError(f"property test not registered: {target}::{name}")
    return executables


def exercise(executable, name, seconds, cases, output):
    started = time.monotonic()
    rounds = 0
    log_path = output / f"{name.replace('::', '-')}.log"
    with log_path.open("w") as log:
        while time.monotonic() - started < seconds:
            seed = rounds + 1
            environment = {**os.environ, "PROPTEST_CASES": str(cases), "PROPTEST_RNG_SEED": str(seed)}
            log.write(f"seed={seed} cases={cases}\n")
            log.flush()
            result = subprocess.run(
                [executable, "--exact", name, "--test-threads=1", "--nocapture"],
                cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT,
                # Bound a stalled batch while permitting complex shrinking to fail visibly.
                timeout=max(60, seconds),
            )
            if result.returncode:
                raise RuntimeError(f"{name} failed at seed {seed}; see {log_path}")
            rounds += 1
    evidence = {"test": name, "elapsed_seconds": time.monotonic() - started,
                "batches": rounds, "cases_per_batch": cases, "requested_cases": rounds * cases,
                "first_seed": 1, "last_seed": rounds, "log": str(log_path)}
    print(json.dumps(evidence), flush=True)
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=positive, default=300)
    parser.add_argument("--cases", type=positive, default=10000)
    parser.add_argument("--jobs", type=positive, default=3)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = args.output or Path(tempfile.mkdtemp(prefix="penelope-property-stress-"))
    output.mkdir(parents=True, exist_ok=True)
    executables = build_tests()
    print(f"Running {len(PROPERTIES)} properties for {args.seconds}s each; evidence: {output}", flush=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = [pool.submit(exercise, executables[target], name, args.seconds, args.cases, output)
                   for target, name in PROPERTIES]
        evidence = [future.result() for future in futures]
    (output / "summary.json").write_text(json.dumps(evidence, indent=2) + "\n")


if __name__ == "__main__":
    main()
