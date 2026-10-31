#!/usr/bin/env bash
# Repeated pure-engine restart, timer-fault, and public-boundary fuzz drill.
#
# This campaign covers library transitions and bounded public-boundary fuzzing.
# SQLite fuzz targets use real private databases; actual adapter process-kill
# suites run separately in workspace tests. This is not a deployment chaos test.
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root_dir}"

iterations="${PENELOPE_CHAOS_ITERATIONS:-3}"
proptest_cases="${PENELOPE_CHAOS_PROPTEST_CASES:-1000}"
fuzz_runs="${PENELOPE_CHAOS_FUZZ_RUNS:-10000}"

for value_name in iterations proptest_cases fuzz_runs; do
  value="${!value_name}"
  if ! [[ "${value}" =~ ^[1-9][0-9]*$ ]]; then
    printf 'PENELOPE_CHAOS_%s must be a positive integer\n' "$(tr '[:lower:]' '[:upper:]' <<<"${value_name}")" >&2
    exit 2
  fi
done

if [[ -n "${PENELOPE_CHAOS_SEED:-}" ]]; then
  if ! [[ "${PENELOPE_CHAOS_SEED}" =~ ^[1-9][0-9]{0,9}$ ]] \
    || (( PENELOPE_CHAOS_SEED > 2147483647 )); then
    printf '%s\n' 'PENELOPE_CHAOS_SEED must be an integer in 1..2147483647' >&2
    exit 2
  fi
fi

for iteration in $(seq 1 "${iterations}"); do
  campaign_seed="${PENELOPE_CHAOS_SEED:-$iteration}"
  printf '[%s/%s] pure transition/restart/timer drill\n' "${iteration}" "${iterations}"
  PROPTEST_CASES="${proptest_cases}" PROPTEST_RNG_SEED="${campaign_seed}" \
    cargo test -p penelope-executor --all-targets --all-features --locked

  printf '[%s/%s] public-boundary malformed-input drill\n' \
    "${iteration}" "${iterations}"
  PENELOPE_FUZZ_RUNS="${fuzz_runs}" PENELOPE_FUZZ_SEED="${campaign_seed}" \
    ./scripts/run_bounded_fuzz.sh
done

printf 'pure chaos drill passed (%s repeated iterations)\n' "${iterations}"
