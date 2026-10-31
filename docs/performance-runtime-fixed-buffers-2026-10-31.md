# Linux runtime fixed-size buffers — 2026-10-31

The runtime now constructs the durable execution-identity digest input and
checkpoint ACK using fixed stack arrays, replacing temporary Vec buffers.
These are hosted Linux adapter changes. The execution-identity format remains
the prefix, scope digest and big-endian generation; ACK remains the eight-byte
magic followed by the 32-byte checkpoint digest.

The isolated protocol allocation executable now launches the real static
protected worker. It polls a checkpoint, commits its bytes and event to SQLite,
then counts allocations strictly around the runtime's ACK call. Worker
construction, journal commits and subsequent fencing are outside that count.
Restoring the original ACK construction from
738b75bb2c945ddd22a004b20d129ea14d048fc2 makes the assertion fail with
**2 allocations**; the new construction passes with **0** on stable and Rust
1.85. The probe also asserts zero allocations for every pending/checkpoint
poll it performs and verifies that the acknowledged worker can complete.
This counter records allocator calls, including allocation through the default
reallocation implementation; it does not measure bytes or peak live memory.

The independent legacy execution-identity oracle reconstructs both the old
scope encoding and identity encoding. It checks 10,000 deterministic scopes
against generations 0, 1, 255, 256 and u32::MAX: **50,000 comparisons per
toolchain**, all equal. Generation zero is an encoding boundary case, not a
new execution authorization.

Validation passed:

- Runtime library tests (13) and integration tests (54), plus the isolated
  allocation executable, on stable and Rust 1.85.
- Strict Clippy for all runtime targets/features, formatting and layer checks.
- CI for the preceding maximum-frame fix, run 41147767081, completed successfully.

[Raw logs and source/artifact hashes](verification-data/2026-10-31-runtime-fixed-buffers/evidence.json)
retain the expected old-ACK failure and successful final checks. Reproduce with
the commands and environment recorded there. The new real-worker portion runs
only on Linux x86-64; the existing portable worker-protocol probes remain.

No timing benchmark was run for this change. Removing these allocations is
concrete work avoided, but does not establish a latency improvement, a new
global slowdown percentage, a complete runtime stack bound, or a fresh full
0.3.0 release qualification.
