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

The follow-up unloadable-bootstrap test uses a static ELF header with no
loadable segment. The first version assumed synchronous rejection; the
[remote run](https://github.com/STEXS-Technologies/penelope/actions/runs/41076659647)
failed that assumption because its kernel accepted the handoff before the
process faulted. The corrected test covers both immediate runtime errors and
asynchronous process faults, with polling before recovery or recovery directly
from the persisted running generation. It requires no completed output, exactly
one generation and confirmed fencing before recovery records an interruption.
An immediate runtime error must also poison further advance. The test does not
interpret successful launch admission as successful application execution.
[Retained failure and corrected local checks](verification-data/2026-10-30-bootstrap-timing/metadata.json)
record the platform-dependent observation. The
[corrected remote check](https://github.com/STEXS-Technologies/penelope/actions/runs/41077837306)
passes all seven CI jobs, including stable and MSRV.
