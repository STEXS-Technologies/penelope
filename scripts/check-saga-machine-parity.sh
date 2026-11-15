#!/usr/bin/env bash
# Compare legacy and prepared facade consumers with immutable 0.4 observations.
set -euo pipefail
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"
parity_dir="$(mktemp -d)"
trap 'rm -rf "$parity_dir"' EXIT
cargo run --manifest-path ci/saga-parity/Cargo.toml --locked > "$parity_dir/legacy.jsonl"
cargo run --manifest-path ci/saga-parity/Cargo.toml --features prepared --locked > "$parity_dir/prepared.jsonl"
cargo run --manifest-path ci/compat-0.4/Cargo.toml --bin protocols --locked > "$parity_dir/protocols.txt"
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
protocols = Path('ci/compat-0.4')
manifest_protocols = json.loads((protocols / 'baseline/manifest.json').read_text())
for filename, field in [('src/protocols.rs', 'fixture_sha256'), ('src/lib.rs', 'imports_sha256'), ('baseline/Cargo.lock', 'lock_sha256')]:
    if hashlib.sha256((protocols / filename).read_bytes()).hexdigest() != manifest_protocols[field]:
        raise SystemExit('legacy protocol fixture changed; requalify against 0.4 explicitly')
packed = (protocols / 'baseline/protocols-v0.4.0.txt.gz').read_bytes()
expected_protocols = gzip.decompress(packed)
if hashlib.sha256(packed).hexdigest() != manifest_protocols['gzip_sha256'] or hashlib.sha256(expected_protocols).hexdigest() != manifest_protocols['output_sha256']:
    raise SystemExit('legacy protocol corpus digest mismatch')
if (Path(sys.argv[1]) / 'protocols.txt').read_bytes() != expected_protocols:
    raise SystemExit('task/timeline bytes, errors or replay differ from 0.4')
print(f"Task/timeline parity passed: {manifest_protocols['rows']} frozen observations match 0.4")
print(f"Saga parity passed: {manifest['rows']} observations match 0.4 through both facade paths")
PY
