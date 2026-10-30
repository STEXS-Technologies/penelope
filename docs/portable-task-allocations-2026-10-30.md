# Portable task allocation checks — 2026-10-30

`crates/penelope-core/tests/task_allocations.rs` is an isolated test executable
(`harness = false`) with a counting system allocator. It measures five portable
`TaskControl` paths using fixed-capacity journal/runtime fixtures and caller-owned
input, output and checkpoint buffers:

- Pure execution: checkpoint, restoration, fencing, restart and completion.
- Effectful execution: checkpoint, restoration, Unknown reconciliation across
  another restoration, authorized restart and completion.
- Rejection of an undersized checkpoint workspace before dispatch.
- Checkpoint migration, restoration and resumed completion.
- Independently supplied completion after interruption and reconciliation.

Every measured path asserts zero allocations, reallocations, deallocations and
requested allocation bytes. Fixture construction and destruction are inside the
measurement window. Printing is outside it. Before measuring, a positive control
checks that allocation, zeroed allocation, reallocation and deallocation are
observed (2 allocations, 1 reallocation, 2 deallocations, 32 requested bytes).
The test executable has no test-harness worker threads.

Reproduce with:

```sh
cargo test -p penelope-core --test task_allocations --no-default-features --locked
cargo test -p penelope-core --test task_allocations --features serde --locked
cargo test -p penelope-core --test task_allocations --release --no-default-features --locked
cargo test -p penelope-core --test task_allocations --release --features serde --locked
```

The five paths report zero for every counter. The debug configurations also pass
on the workspace MSRV, Rust 1.85.0. CI runs both debug feature configurations.
The separate RISC-V no-std compilation checks remain enabled.

This is runtime regression evidence for these paths and fixtures, not a proof
about every input or every compiler optimization. Rust may eliminate allocations;
the positive control checks the instrumentation in each executable. The fixtures
supply trusted migration/reconciliation evidence and do not establish its
cryptographic validity. This check does not measure serialization, hosted
adapters, SQLite, Linux workers, stack high-water use, peak resident memory,
or worst-case control memory. Those require separate qualification.
