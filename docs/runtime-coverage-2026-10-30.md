# Runtime and failure-path coverage — 2026-10-30

The normal workspace coverage command now reports **8,809 / 9,363 lines covered
(94.0831%)**, passing the unchanged **94.0759%** ratchet. No coverage exclusion or
baseline was changed. This narrow margin should not be interpreted as release
completion or exhaustive fault coverage.

The Linux setup sequence was extracted into one private function, called in the
same order by the existing pre-exec closure. It retains the no-allocation,
no-locking, syscall-only restriction. Successful exec discards the child's LLVM
counters, so separate subprocess tests now exercise that same setup function and
exit normally. They verify:

- Kernel PID/start-time identity is written at the registry offset.
- CPU, address-space and core limits match the configured hard and soft limits.
- An explicitly non-CLOEXEC descriptor becomes CLOEXEC, and parent-death signal
  is SIGKILL.
- Wrong-parent and read-only-manifest failures preserve identity bytes, limits
  and descriptor flags; a real file-size limit induces a short identity write
  and yields EIO before limit/descriptor setup.
- Invalid spawn budgets reject before executing a command or writing a manifest.

Additional tests cover bounded IPC partial writes, interrupted/would-block retry,
zero writes, permanent failure, expired/unrepresentable deadlines and truncated
frame numbers; an unrepresentable fence deadline preserves failure even after
signaling an owned child. Portable tests reject stale and zero-schema observations
without accepting output, and timeout polling records a fault instead of late
completion. SQLite tests exercise invalid/exhausted lease counters, clock overflow,
rollback of pending work and explicit structural integrity failure/repair.

The coverage campaign includes the full normal all-feature workspace test suite.
Affected runtime tests also pass on Rust 1.85.0, as do the portable failure tests
and the new SQLite cases. Workspace Clippy, formatting, layer/action-pin checks,
Python tests and the five pinned upstream signed-source tests pass. The upstream
suite includes its 136 coordinator SIGKILL/recovery cases.

Reproduce:

```sh
./scripts/run_coverage_json.sh
./scripts/check_coverage_ratchet.sh
```

[Retained report and metadata](verification-data/2026-10-30-runtime-coverage/)
include the original summary JSON, report hash, tool versions, baseline,
changed-source hashes and tracked diff from `9039c09`. Subprocess setup tests
qualify the shared operations; they do not measure every post-fork error or make
exec-path counters survive. Kernel seccomp and real worker recovery remain
covered by the separate runtime integration suite. Performance, upstream proof
integration and the remaining release gates still require qualification.
