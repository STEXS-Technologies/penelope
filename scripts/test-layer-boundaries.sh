#!/usr/bin/env bash
# Exercise the real gate with synthetic Cargo metadata; no dependency is fetched.
set -euo pipefail

gate_test_dir="$(mktemp -d)"
trap 'rm -rf "$gate_test_dir"' EXIT
cargo metadata --format-version=1 --no-deps > "$gate_test_dir/baseline.json"
cp "$gate_test_dir/baseline.json" "$gate_test_dir/metadata.json"
export gate_test_dir
cargo() { cat "$gate_test_dir/metadata.json"; }
export -f cargo

./scripts/check-layer-boundaries.sh
for forbidden_crate in aws-sdk-s3 aws-sdk-kms sqlx; do
  python3 - "$gate_test_dir" "$forbidden_crate" <<'PY'
import json, sys
from pathlib import Path
directory, dependency = Path(sys.argv[1]), sys.argv[2]
metadata = json.loads((directory / 'baseline.json').read_text())
package = next(p for p in metadata['packages'] if p['name'] == 'penelope-ports')
package['dependencies'].append({'name': dependency})
(directory / 'metadata.json').write_text(json.dumps(metadata))
PY
  if ./scripts/check-layer-boundaries.sh > "$gate_test_dir/result.txt" 2>&1; then
    printf 'gate incorrectly admitted %s\n' "$forbidden_crate" >&2
    exit 1
  fi
  if ! grep -Fq 'production infrastructure dependency found' "$gate_test_dir/result.txt"; then
    cat "$gate_test_dir/result.txt" >&2
    exit 1
  fi
done
for dependency_kind in dev normal build; do
  python3 - "$gate_test_dir" "$dependency_kind" <<'PY_KIND'
import json, sys
from pathlib import Path
directory, kind = Path(sys.argv[1]), sys.argv[2]
metadata = json.loads((directory / 'baseline.json').read_text())
package = next(p for p in metadata['packages'] if p['name'] == 'penelope-executor')
package['dependencies'].append({'name': 'rusqlite', 'kind': None if kind == 'normal' else kind})
(directory / 'metadata.json').write_text(json.dumps(metadata))
PY_KIND
  if [[ "$dependency_kind" == dev ]]; then
    ./scripts/check-layer-boundaries.sh
  elif ./scripts/check-layer-boundaries.sh > "$gate_test_dir/result.txt" 2>&1; then
    printf 'gate incorrectly admitted %s infrastructure dependency\n' "$dependency_kind" >&2
    exit 1
  elif ! grep -Fq 'production infrastructure dependency found' "$gate_test_dir/result.txt"; then
    cat "$gate_test_dir/result.txt" >&2
    exit 1
  fi
done
printf '%s\n' 'Layer gate permits test-only infrastructure and rejects normal/build clients' 
