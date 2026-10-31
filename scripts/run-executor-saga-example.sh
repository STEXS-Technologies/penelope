#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
example_dir="$(mktemp -d)"
trap 'rm -rf "$example_dir"' EXIT
cargo run -p penelope-executor --example saga_recoverable_task --locked -- \
  "$example_dir/state" | tee "$example_dir/first.log"
grep -Fqx 'Verified sum: 6; simulated task completed the durable saga' "$example_dir/first.log"
cargo run -p penelope-executor --example saga_recoverable_task --locked -- \
  "$example_dir/state" | tee "$example_dir/replayed.log"
grep -Fqx 'Saga already completed; no task dispatched' "$example_dir/replayed.log"
