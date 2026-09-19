#!/usr/bin/env bash
set -euo pipefail

# Generate the same LLVM source-based JSON report used by the coverage CI job.
# The fuzz workspace member is intentionally excluded from the library metric.
cargo llvm-cov \
  --workspace \
  --all-features \
  --locked \
  --exclude penelope-fuzz \
  --json \
  --summary-only \
  --output-path target/coverage.json \
  "$@"
