#!/usr/bin/env python3
"""Run a paired 0.4.0-versus-current SQLite workload comparison.

The full 31-pair run is intentionally explicit and retains every benchmark
directory. Run it only after concurrent pinned-CPU qualification work has ended.
"""
import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[4]
BASELINE = Path("/tmp/penelope-v040-baseline")
OUTPUT = Path("/tmp/penelope-050-sqlite-post-qpsg-candidate")
DATA = ROOT / "target/perf-data-post-qpsg"


def run(*args, cwd=None, env=None, capture_output=False):
    return subprocess.run(args, cwd=cwd, env=env, check=True, text=True,
                          capture_output=capture_output)


def revision(path):
    return run("git", "rev-parse", "HEAD", cwd=path,
               capture_output=True).stdout.strip()


def main():
    if not BASELINE.is_dir():
        raise SystemExit(f"missing frozen baseline checkout: {BASELINE}")
    if OUTPUT.exists():
        raise SystemExit(f"output already exists: {OUTPUT}")
    tracked_changes = set(run("git", "diff", "--name-only", "HEAD", cwd=ROOT,
                              capture_output=True).stdout.splitlines())
    documentation_only = {"docs/0.5/verification.md"}
    if tracked_changes - documentation_only:
        raise SystemExit(f"unexpected tracked changes in candidate: {sorted(tracked_changes)}")
    for path in (ROOT, BASELINE):
        if path == BASELINE and run("git", "diff", "HEAD", "--", cwd=path,
                                    capture_output=True).stdout:
            raise SystemExit(f"tracked changes present in {path}")
    source = "crates/penelope-sqlite/benches/durable_store.rs"
    if (ROOT / source).read_bytes() != (BASELINE / source).read_bytes():
        raise SystemExit("baseline and candidate durable-store fixtures differ")
    DATA.mkdir(parents=True, exist_ok=False)
    OUTPUT.mkdir(parents=True)
    plan = {
        "schema": 1,
        "purpose": "Post-QPSG candidate SQLite performance review against original 0.4.0.",
        "revisions": {"before": revision(BASELINE), "after": revision(ROOT)},
        "pairs": 31,
        "warmup_pairs": 1,
        "order": "Alternating before/after and after/before pairs; warmup pair retained but excluded.",
        "fixture_sha256": __import__("hashlib").sha256((ROOT / source).read_bytes()).hexdigest(),
        "cpu": 4,
        "toolchain": "1.100.0",
        "storage": "Same NVMe/Btrfs data parent; SQLite WAL/FULL; OS cache uncontrolled; no fsync bypass.",
        "concurrent_work": "Do not overlap the pinned full chaos campaign or another benchmark on CPU 4.",
        "environment": {key: os.environ.get(key) for key in
                        ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS")},
        "candidate_documentation_changes": sorted(tracked_changes),
    }
    (OUTPUT / "comparison.json").write_text(json.dumps(plan, indent=2) + "\n")
    environment = {**os.environ, "RUSTC_WRAPPER": "",
                   "RUSTC_WORKSPACE_WRAPPER": "", "CARGO_BUILD_JOBS": "2"}
    os.sched_setaffinity(0, {4})
    for pair in range(32):
        labels = ["before", "after"] if pair % 2 == 0 else ["after", "before"]
        for label in labels:
            repository = BASELINE if label == "before" else ROOT
            prefix = "warmup" if pair == 0 else f"pair-{pair:02d}"
            name = f"{prefix}-{label}"
            target = ROOT / f"target/sqlite-post-qpsg-{label}"
            env = {**environment, "CARGO_TARGET_DIR": str(target)}
            log_path = OUTPUT / f"{name}.log"
            command = [sys.executable, str(ROOT / "scripts/benchmark-durable-store.py"),
                       str(OUTPUT / name), "--repository", str(repository),
                       "--data-parent", str(DATA), "--cpu", "4", "--runs", "1",
                       "--benchmark", "durable_store", "--toolchain", "1.100.0"]
            print(f"Running {name}", flush=True)
            with log_path.open("w") as log:
                subprocess.run(command, cwd=ROOT, env=env, stdout=log,
                               stderr=subprocess.STDOUT, check=True, text=True)
        print(f"Completed pair {pair}", flush=True)
    print(f"Raw comparison retained at {OUTPUT}", flush=True)


if __name__ == "__main__":
    main()
