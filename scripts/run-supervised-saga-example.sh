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

# The same protected runtime also completes a task admitted by an application
# machine, then returns its verified output through the machine journal.
cargo run --manifest-path ci/machine-consumer/Cargo.toml --bin durable \
  --features protected --locked -- "$example_dir/worker" | tee "$example_dir/machine.log"
grep -Fqx 'task_result_verified=true amount=1' "$example_dir/machine.log"
grep -Fqx 'result_committed=true state=257 revision=2' "$example_dir/machine.log"
grep -Fqx 'saga_admitted=true native_actions=1 source_acknowledged=true' "$example_dir/machine.log"

# A completed worker is not sufficient: a different application result must
# stop before the machine result event, even when its code digest is pinned.
sed 's/u64 sum=0;/u64 sum=1;/' crates/penelope-process-runtime/tests/fixtures/worker.c > "$example_dir/wrong-result.c"
cc -nostdlib -static -fno-builtin -fno-stack-protector -fno-pie -no-pie \
  -o "$example_dir/wrong-result" "$example_dir/wrong-result.c"
if cargo run --manifest-path ci/machine-consumer/Cargo.toml --bin durable \
  --features protected --locked -- "$example_dir/wrong-result" > "$example_dir/rejected.log" 2>&1; then
  echo "unexpectedly accepted the wrong task result" >&2
  exit 1
fi
grep -F 'task result schema or meaning differs; no result event admitted' "$example_dir/rejected.log"
if grep -Fq 'result_committed=true' "$example_dir/rejected.log"; then
  echo "wrong task result changed the application machine" >&2
  exit 1
fi
echo 'Wrong task result refused before machine admission'
