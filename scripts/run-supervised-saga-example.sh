#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  echo "The supervised saga example requires Linux x86-64" >&2
  exit 1
fi
command -v cc >/dev/null
example_dir="$(mktemp -d)"
trap 'rm -rf "$example_dir"' EXIT

# A freestanding worker needs no libc or target downloads. The same fixture is
# exercised by the runtime's syscall, protocol and crash integration suites.
cc -nostdlib -static -fno-builtin -fno-stack-protector -fno-pie -no-pie \
  -o "$example_dir/worker" crates/penelope-process-runtime/tests/fixtures/worker.c
cargo run -p penelope-process-runtime --example saga_recoverable_task --locked -- \
  "$example_dir/state" "$example_dir/worker" | tee "$example_dir/first.log"
grep -Fqx 'Verified sum: 6; saga completed in separate durable stores' "$example_dir/first.log"
cargo run -p penelope-process-runtime --example saga_recoverable_task --locked -- \
  "$example_dir/state" "$example_dir/worker" | tee "$example_dir/replayed.log"
grep -Fqx 'Saga already completed; no task dispatched' "$example_dir/replayed.log"
