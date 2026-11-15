#!/usr/bin/env bash
# Compare legacy and prepared facade consumers with immutable 0.4 observations.
set -euo pipefail
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"
parity_dir="$(mktemp -d)"
trap 'rm -rf "$parity_dir"' EXIT
cargo run --manifest-path ci/saga-parity/Cargo.toml --locked > "$parity_dir/legacy.jsonl"
cargo run --manifest-path ci/saga-parity/Cargo.toml --features prepared --locked > "$parity_dir/prepared.jsonl"
python3 - "$parity_dir" <<'PY'
import gzip, hashlib, json, sys
from pathlib import Path
root = Path('ci/saga-parity')
manifest = json.loads((root / 'baseline/manifest.json').read_text())
if hashlib.sha256((root / 'src/main.rs').read_bytes()).hexdigest() != manifest['fixture_sha256']:
    raise SystemExit('fixture changed; qualify new observations against 0.4 explicitly')
expected = gzip.decompress((root / 'baseline/v0.4.0.jsonl.gz').read_bytes())
if hashlib.sha256(expected).hexdigest() != manifest['output_sha256']:
    raise SystemExit('baseline output digest mismatch')
for name in ['legacy', 'prepared']:
    actual = (Path(sys.argv[1]) / (name + '.jsonl')).read_bytes()
    if actual != expected:
        raise SystemExit(name + ' decisions, errors or replay records differ from 0.4')
print(f"Saga parity passed: {manifest['rows']} observations match 0.4 through both facade paths")
PY
