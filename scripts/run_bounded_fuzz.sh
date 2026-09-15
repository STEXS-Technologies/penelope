#!/usr/bin/env bash
# Runs and structurally verifies the complete bounded public-boundary fuzz set.
#
# This is intentionally library-only: it checks parsers, DTO contracts, pure
# decisions, and ports. It does not claim durable-adapter or deployment chaos
# coverage.
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root_dir}"

fuzz_runs="${PENELOPE_FUZZ_RUNS:-1000}"
if ! [[ "${fuzz_runs}" =~ ^[1-9][0-9]*$ ]]; then
  printf '%s\n' 'PENELOPE_FUZZ_RUNS must be a positive integer' >&2
  exit 2
fi

fuzz_targets=(
  fuzz_identifiers
  fuzz_dtos
  fuzz_linear_engine
  fuzz_statechronicle_correlation
  fuzz_atomic_process_commit
  fuzz_canonical_reconciliation
  fuzz_manual_review_lifecycle
  fuzz_process_input_parse
  fuzz_process_control_ports
)

dtos=(
  ProcessDefinition
  ProcessInput
  ProcessInputEnvelope
  ProcessOutcome
  ProcessAction
  CanonicalCommand
  CanonicalEvent
  ManualReview
)

for fuzz_target in "${fuzz_targets[@]}"; do
  if ! grep -Fq "name = \"${fuzz_target}\"" fuzz/Cargo.toml; then
    printf 'missing required fuzz target registration: %s\n' "${fuzz_target}" >&2
    exit 1
  fi
  if [[ ! -f "fuzz/fuzz_targets/${fuzz_target}.rs" ]]; then
    printf 'missing required fuzz target source: %s\n' "${fuzz_target}" >&2
    exit 1
  fi
done

for dto in "${dtos[@]}"; do
  if ! grep -Fwq "${dto}" fuzz/fuzz_targets/fuzz_dtos.rs; then
    printf 'missing typed protocol value fuzz coverage: %s\n' "${dto}" >&2
    exit 1
  fi
done

for fuzz_target in "${fuzz_targets[@]}"; do
  printf '%s malformed-input drill (%s runs)\n' "${fuzz_target}" "${fuzz_runs}"
  cargo +nightly fuzz run "${fuzz_target}" -- -runs="${fuzz_runs}"
done
