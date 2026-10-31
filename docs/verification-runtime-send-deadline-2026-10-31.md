# Linux IPC send deadlines — 2026-10-31

The Linux runtime now enforces one deadline across the complete startup-frame
write. Checkpoint acknowledgements also reject successful writes that finish
late. A timed-out transfer remains ambiguous: bytes already written cannot be
recalled, and the controller requires reconstruction and fencing.

## Reproduced failure and change

Baseline c6a260cacc36450f7dab46baddbc5e23a03fff56 checks the send deadline only
following WouldBlock or Interrupted. A writer making continuous successful short
writes can exceed the timeout and still return success, including on its final
write. Both regression tests failed on that baseline with actual Ok(()) versus
expected Runtime. Startup additionally gave each of its four parts a fresh
budget.

Source 0e8a66d adds a shared deadline for startup header, metadata, input and
optional checkpoint. Every nonempty part checks that deadline before writing
and after its final positive write. This preserves partial-write offsets,
transient-error retry behavior, permanent-error handling, empty-part behavior
and overflow rejection. No wire encoding, durable schema, public signature,
portable-core code or dependency changed.

The timeout covers the IPC write phase, not image loading or process creation.
Nonblocking descriptors prevent waiting inside production writes; the host
scheduler can still delay execution or detection of an expired deadline. Small
configured timeouts can now reject transfers that previously finished late.
This adds monotonic-clock checks to writes; no performance gain or unchanged
end-to-end overhead is claimed.

## Verification

Four new unit tests cover positive-progress expiry, final-write expiry,
expired-part rejection without writing, and cumulative startup parts sharing
one budget. The slow writer sleeps two milliseconds per positive byte; deadlines
are one or three milliseconds. Assertions avoid an exact elapsed-time or
positive-prefix-length requirement. Existing retry, broken-pipe, zero-write and
overflow tests remain in place.

A new real protected-worker test sets a one-nanosecond IPC write budget. It
verifies a Runtime error after durable start, reconstruction-required poisoning,
reconstructed interruption with no output, termination fencing and rejection
of an old-generation start. It retains the original child handle until after
reconstruction proves termination, then fences it to reap the child.

- Stable full workspace/all targets/all features passed, excluding the fuzz
  package. That run preceded the additional recovery test; the final runtime
  suite was rerun and passed 17 library and 55 integration tests plus benchmarks
  and the example target.
- Rust 1.85.0 full workspace/all targets/all features was rerun with
  RUST_TEST_THREADS=1, excluding fuzz, with final source and the recovery test.
  The initial parallel run timed out in two existing coordinator-recovery
  subprocesses (ten-second limits). Both passed a separate serial retry.
  A first whole-workspace serial invocation used a command-line thread flag,
  which the custom allocation harness rejected; the environment setting avoids
  passing unsupported arguments to that harness. The final serial
  full-workspace run passed; its logs are retained.
- Strict workspace Clippy, formatting, layer checks and AST graph update passed.
- On each compiler, 50 additional repetitions ran four bounded-send tests, two
  shared-part tests and the real recovery test: 350 test executions in 150
  processes per compiler, 700 executions total.

Workspace benchmark executions here are smoke tests, not performance evidence.
No additional fuzz campaign was run: the changed boundary is elapsed-time and
write-result handling, covered by the write fixtures and protected-worker test.

[Retained evidence](verification-data/2026-10-31-runtime-send-deadline/evidence.json)
contains source/toolchain pins, commands, hashes, baseline fixture patch,
compressed baseline/final validation logs and all repeated-process outputs.
The fixture patch reproduces the two failing baseline assertions with canonical
formatting. The first lint attempt found test-variable shadowing; the final
strict check passed after the variables were renamed. Those initial diagnostics
are retained too.
