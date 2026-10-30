# Worker executable-loading boundary

The Linux worker filter now permits only the bootstrap `execveat` call on a
parent-prepared duplicate of the sealed image descriptor, with `AT_EMPTY_PATH`.
The descriptor is above standard input/output/error and closes on successful
exec. Ordinary `execve`, alternate descriptors/flags, descriptor creation and
descriptor duplication are denied. All BPF construction and descriptor
allocation happen before fork; the child executes through fixed C strings and
stack pointer arrays with no allocator, formatting or application callback.

The earlier implementation allowed `execve` for Rust's normal command startup,
which also left it available to the running worker. The same new escape probe
against `a15adf3` fails: syscall tracing confirms successful
`execve("/proc/self/exe", NULL, NULL)`, after which the worker waits for a second
input frame and the test times out. This was a real missing restriction, not a
claim that the existing filter's network/process/limit restrictions disappeared.

With the correction, the trace confirms successful sealed-descriptor bootstrap
and SIGSYS termination for subsequent `execve`, path-based `execveat` and `dup2`
attempts. Durable task history records `ProtectionViolation`; none completes.
The original positive cases still complete through the exact bounded protocol.
The full process-runtime all-target suite passes on Rust 1.99.0 and 1.85.0,
along with strict Clippy. The real-worker saga example and all seven pinned
StateChronicle signed-source/checkpoint tests pass, including their process-kill
matrices. These are correctness traces, not qualified latency measurements.

[Retained logs, traces and source hashes](verification-data/2026-10-30-sealed-exec/metadata.json)
include the failing baseline and successful corrected probe. Existing dispatch
latency reports describe their earlier pinned candidates; the final worker
measurement must include this changed startup path. The library state-machine
fuzz campaign continues independently; final workspace checks and runtime
measurements remain required before release qualification.
