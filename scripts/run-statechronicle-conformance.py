#!/usr/bin/env python3
"""Run optional signed-event and checkpoint-state proof conformance against a pinned checkout."""
import argparse
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import tempfile

PINNED_REVISION = "ae3d94bc22c7835eaa3fdcd766e3c117929c12bb"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("statechronicle", type=Path)
    parser.add_argument("--refresh-lock", action="store_true", help="explicitly regenerate the checked-in conformance lockfile")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("the real-worker signed-source conformance profile requires Linux x86_64")
    upstream = args.statechronicle.resolve()
    revision = subprocess.check_output(["git", "-C", str(upstream), "rev-parse", "HEAD"], text=True).strip()
    if revision != PINNED_REVISION:
        parser.error(f"expected StateChronicle {PINNED_REVISION}, found {revision}")
    if subprocess.check_output(["git", "-C", str(upstream), "diff", "HEAD", "--", "crates", "Cargo.toml", "Cargo.lock"], text=True):
        parser.error("tracked upstream sources must match the pinned revision")
    root = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="penelope-statechronicle-") as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text("// Nonpublishable conformance package.\n")
        (project / "tests").mkdir()
        shutil.copyfile(root / "scripts/conformance/statechronicle.rs", project / "tests/signed_source.rs")
        shutil.copyfile(root / "scripts/conformance/task_sagas.rs", project / "tests/task_sagas.rs")
        shutil.copyfile(root / "scripts/conformance/checkpoint_proof.rs", project / "tests/checkpoint_proof.rs")
        runtime_tests = root / "crates/penelope-process-runtime/tests"
        shutil.copytree(runtime_tests / "support", project / "tests/support")
        shutil.copytree(runtime_tests / "fixtures", project / "tests/fixtures")
        manifest = ['[package]', 'name = "penelope-statechronicle-conformance"', 'version = "0.0.0"', 'edition = "2024"', 'publish = false', '[workspace]', '[dependencies]', 'async-trait = "0.1"', 'libc = "0.2"', 'serde = { version = "1", features = ["derive"] }', 'chrono = "0.4"', 'ed25519-dalek = "2"', 'serde_json = "1"', 'tempfile = "3"', 'rusqlite = { version = "0.40.2", features = ["bundled"] }']
        for name in ["penelope-core", "penelope-domain", "penelope-ports", "penelope-executor", "penelope-process-runtime", "penelope-statechronicle", "penelope-sqlite"]:
            manifest.append(f'{name} = {{ path = {json.dumps(str(root / "crates" / name))} }}')
        for name in ["statechronicle-core", "statechronicle-domain", "statechronicle-commit", "statechronicle-accumulator", "statechronicle-proof"]:
            manifest.append(f'{name} = {{ path = {json.dumps(str(upstream / "crates" / name))} }}')
        (project / "Cargo.toml").write_text("\n".join(manifest) + "\n")
        lockfile = root / "scripts/conformance/Cargo.lock"
        if args.refresh_lock:
            subprocess.run(["cargo", "+1.99.0", "generate-lockfile", "--manifest-path", str(project / "Cargo.toml")], check=True)
            shutil.copyfile(project / "Cargo.lock", lockfile)
        else:
            shutil.copyfile(lockfile, project / "Cargo.lock")
        print(f"StateChronicle revision: {revision}", flush=True)
        test_command = ["cargo", "+1.99.0", "test", "--manifest-path", str(project / "Cargo.toml"), "--locked", "--test", "signed_source", "signed_"]
        environment = {**os.environ, "CARGO_TARGET_DIR": str(root / "target/statechronicle-conformance")}
        listing = subprocess.run([*test_command, "--", "--list"], text=True, capture_output=True, env=environment)
        if listing.returncode:
            print(listing.stderr, flush=True)
            listing.check_returncode()
        expected_tests = {
            "signed_committed_source_binds_atomic_commit_and_deduplicates_after_reopen",
            "task_sagas::signed_source_drives_pure_linear_and_graph_sagas_in_both_store_layouts",
            "task_sagas::signed_source_drives_reconciled_linear_and_graph_sagas_in_both_store_layouts",
            "task_sagas::signed_source_pure_sagas_recover_after_sigkill",
            "task_sagas::signed_source_effectful_sagas_recover_after_sigkill",
            "checkpoint_proof::signed_checkpoint_proof_rejects_substitution_and_stale_authority",
            "checkpoint_proof::signed_checkpoint_proof_gates_real_worker_commit_ack_and_restore",
        }
        discovered = {line.removesuffix(": test") for line in listing.stdout.splitlines() if line.endswith(": test")}
        if discovered != expected_tests:
            raise RuntimeError(f"unexpected signed-source test selection: {discovered}")
        subprocess.run(test_command, check=True, env=environment)


if __name__ == "__main__":
    main()
