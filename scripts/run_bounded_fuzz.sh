#!/usr/bin/env bash
# Runs and structurally verifies the complete bounded public-boundary fuzz set.
#
# This is intentionally library-only: it checks parsers, DTO contracts, pure
# decisions, ports, and bounded SQLite authority/process operation sequences. SQLite
# runs use private temporary local files; this is not deployment chaos coverage.
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root_dir}"

fuzz_runs="${PENELOPE_FUZZ_RUNS:-1000}"
if ! [[ "${fuzz_runs}" =~ ^[1-9][0-9]*$ ]]; then
  printf '%s\n' 'PENELOPE_FUZZ_RUNS must be a positive integer' >&2
  exit 2
fi

# Optional explicit seeds make repeated campaigns reproducible. With no seed,
# libFuzzer retains its normal randomized selection.
fuzz_options=(-runs="${fuzz_runs}")
if [[ -n "${PENELOPE_FUZZ_SEED:-}" ]]; then
  if ! [[ "${PENELOPE_FUZZ_SEED}" =~ ^[1-9][0-9]{0,9}$ ]] \
    || (( PENELOPE_FUZZ_SEED > 2147483647 )); then
    printf '%s\n' 'PENELOPE_FUZZ_SEED must be an integer in 1..2147483647' >&2
    exit 2
  fi
  fuzz_options+=(-seed="${PENELOPE_FUZZ_SEED}")
fi

fuzz_targets=(
  fuzz_newtypes
  fuzz_identifiers
  fuzz_dtos
  fuzz_linear_engine
  fuzz_statechronicle_correlation
  fuzz_atomic_process_commit
  fuzz_canonical_reconciliation
  fuzz_manual_review_lifecycle
  fuzz_process_input_parse
  fuzz_process_control_ports
  fuzz_task_history
  fuzz_task_supervisor
  fuzz_task_checkpoint
  fuzz_timeline_wire
  fuzz_timeline_projection
  fuzz_timeline_replay_plan
  fuzz_timeline_correction
  fuzz_timeline_branch_promotion
  fuzz_timeline_effects
  fuzz_worker_protocol
  fuzz_sqlite_task_authority
  fuzz_sqlite_process
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

python3 scripts/seed-timeline-corpus.py

for fuzz_target in "${fuzz_targets[@]}"; do
  printf '%s malformed-input drill (%s runs)\n' "${fuzz_target}" "${fuzz_runs}"
  cargo +nightly fuzz run "${fuzz_target}" -- "${fuzz_options[@]}"
done
