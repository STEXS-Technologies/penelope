#!/usr/bin/env python3
"""Run optional signed-event conformance against a pinned installed checkout."""
import argparse
import json
import os
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
        manifest = ['[package]', 'name = "penelope-statechronicle-conformance"', 'version = "0.0.0"', 'edition = "2024"', 'publish = false', '[workspace]', '[dependencies]', 'chrono = "0.4"', 'ed25519-dalek = "2"', 'serde_json = "1"', 'tempfile = "3"', 'rusqlite = { version = "0.40.2", features = ["bundled"] }']
        for name in ["penelope-domain", "penelope-ports", "penelope-statechronicle", "penelope-sqlite"]:
            manifest.append(f'{name} = {{ path = {json.dumps(str(root / "crates" / name))} }}')
        for name in ["statechronicle-core", "statechronicle-domain", "statechronicle-commit"]:
            manifest.append(f'{name} = {{ path = {json.dumps(str(upstream / "crates" / name))} }}')
        (project / "Cargo.toml").write_text("\n".join(manifest) + "\n")
        lockfile = root / "scripts/conformance/Cargo.lock"
        if args.refresh_lock:
            subprocess.run(["cargo", "+1.99.0", "generate-lockfile", "--manifest-path", str(project / "Cargo.toml")], check=True)
            shutil.copyfile(project / "Cargo.lock", lockfile)
        else:
            shutil.copyfile(lockfile, project / "Cargo.lock")
        print(f"StateChronicle revision: {revision}", flush=True)
        subprocess.run(["cargo", "+1.99.0", "test", "--manifest-path", str(project / "Cargo.toml"), "--locked"], check=True,
                       env={**os.environ, "CARGO_TARGET_DIR": str(root / "target/statechronicle-conformance")})


if __name__ == "__main__":
    main()
