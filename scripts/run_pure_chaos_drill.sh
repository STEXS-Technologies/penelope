#!/usr/bin/env bash
# Repeated pure-engine restart, timer-fault, and public-boundary fuzz drill.
#
# This is deliberately not a deployment/database chaos test: Penelope ships no
# infrastructure adapter. It makes the library-side failure evidence repeatable
# until an outer composition root can run the corresponding durable drills.
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

for iteration in $(seq 1 "${iterations}"); do
  printf '[%s/%s] pure transition/restart/timer drill\n' "${iteration}" "${iterations}"
  PROPTEST_CASES="${proptest_cases}" \
    cargo test -p penelope-executor --all-targets --all-features --locked

  printf '[%s/%s] public-boundary malformed-input drill\n' \
    "${iteration}" "${iterations}"
  PENELOPE_FUZZ_RUNS="${fuzz_runs}" ./scripts/run_bounded_fuzz.sh
done

printf 'pure chaos drill passed (%s repeated iterations)\n' "${iterations}"
