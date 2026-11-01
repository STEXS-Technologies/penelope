#!/usr/bin/env bash
# Verify the actual campaign scripts without building or fuzzing: mocked cargo
# records structured arguments and seed environments for independent assertions.
set -euo pipefail
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root_dir}"
campaign_test_dir="$(mktemp -d)"
trap 'rm -rf "$campaign_test_dir"' EXIT
export campaign_test_dir
cargo() {
  python3 - "$@" <<'PY'
import json, os, sys
from pathlib import Path
with (Path(os.environ['campaign_test_dir']) / 'calls.jsonl').open('a') as log:
    log.write(json.dumps({'args': sys.argv[1:], 'seed': os.getenv('PROPTEST_RNG_SEED'),
                         'cases': os.getenv('PROPTEST_CASES')}) + '\n')
PY
}
export -f cargo
unset PENELOPE_CHAOS_SEED PENELOPE_FUZZ_SEED
PENELOPE_CHAOS_ITERATIONS=3 PENELOPE_CHAOS_PROPTEST_CASES=2 PENELOPE_CHAOS_FUZZ_RUNS=1 \
  ./scripts/run_pure_chaos_drill.sh > "$campaign_test_dir/output.txt"
python3 - <<'PY'
import json, os, tomllib
from pathlib import Path
calls = [json.loads(line) for line in (Path(os.environ['campaign_test_dir']) / 'calls.jsonl').read_text().splitlines()]
targets = {entry['name'] for entry in tomllib.loads(Path('fuzz/Cargo.toml').read_text())['bin']}
assert len(calls) == 3 * (1 + len(targets)), calls
for iteration in range(3):
    block = calls[iteration * (1 + len(targets)):(iteration + 1) * (1 + len(targets))]
    assert block[0]['args'][:5] == ['test', '-p', 'penelope-executor', '-p', 'penelope-timeline'], block
    assert block[0]['seed'] == str(iteration + 1), block[0]
    assert block[0]['cases'] == '2', block[0]
    assert {call['args'][3] for call in block[1:]} == targets, block
    for call in block[1:]:
        assert call['args'][:3] == ['+nightly', 'fuzz', 'run'], call
        assert call['args'][4:] == ['--', '-runs=1', f'-seed={iteration + 1}'], call
PY
for invalid_seed in 0 -1 2147483648 999999999999999999999999 01 '1 2' garbage; do
  for runner in run_bounded_fuzz.sh run_pure_chaos_drill.sh; do
    if PENELOPE_FUZZ_SEED="$invalid_seed" PENELOPE_CHAOS_SEED="$invalid_seed" \
      "./scripts/$runner" > "$campaign_test_dir/rejection.txt" 2>&1; then
      printf 'runner admitted invalid seed: %s (%s)\n' "$invalid_seed" "$runner" >&2
      exit 1
    fi
  done
done
printf '%s\n' 'Chaos runners cover every registered target with matching explicit seeds'
