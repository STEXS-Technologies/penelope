# penelope-process-runtime

Optional Linux process execution for Penelope's task ports. The default facade
does not depend on this package. The [0.3.0 qualification report](../../docs/qualification-0.3.0.md)
records the verified profile, evidence and limits.

## Supported profile

`LinuxProcessRuntime` supports static Linux x86-64 ELF workers running as one
process and one thread. It requires a trusted Linux kernel with executable sealed
memfd support (`MFD_EXEC`, Linux 6.3 or newer), seccomp filters, pidfds
(including `waitid(P_PIDFD)`),
`close_range`, procfs, and a private local registry directory. Unsupported images
or unavailable kernel facilities fail closed. Dynamic loaders are rejected.

Worker bytes are copied into a sealed executable memory file; `code_identity()`
is the digest to place in the task definition. Workers receive only bounded
stdin/stdout protocol frames. A syscall allowlist prevents opening files,
networking, creating processes/threads, and signalling other processes. This
profile does not support arbitrary existing programs. No container or cgroup
setup is performed.

Startup uses `execveat` only on a duplicate of the sealed executable descriptor
with `AT_EMPTY_PATH`. That descriptor closes on successful exec. The worker's
filter rejects `execve`, alternative `execveat` descriptors/flags and descriptor
creation/duplication, so it cannot reopen the executable-loading boundary after
startup. The supported kernel facilities include `execveat`.


CPU limits must be positive whole seconds expressed at the configured trusted
tick frequency. The kernel enforces CPU time independently of supervisor polls.
An address-space limit bounds virtual memory, not resident memory. Configure
frame sizes, live worker count, startup/fence timeouts, and executable size.
Application deadlines still require the trusted supervisor clock and driver.

## Recovery contract

Inject a durable `TaskGenerationAuthority`, such as `SqliteTaskJournal`, and use
the same private registry across reconstruction. Launch reserves the generation
before creating a child. Before executing worker code the child durably records
its PID, kernel start time and host boot identity. Launch and fencing share a
registry lock. Fencing persists its tombstone first, verifies process identity,
kills through a pidfd, and confirms termination before acknowledging the fence.
Errors leave the generation blocked for recovery. When this host process is
still the worker's parent, fencing also reaps the stopped child through its
identity-checked pidfd, including after the original runtime was dropped.
Workers owned by another parent are reaped by that parent. An original runtime
can retry fencing after another host reaper has already consumed its child's
exit status; it still requires proof that its recorded identity is stopped.

Keep the registry with its corresponding database. Do not delete manifests or
change registry paths while generations may exist. Filesystem ownership and
host access control are deployment responsibilities. Registry records are not
cryptographic authentication against another process with write access.

Linux parent-death signals attach to the spawning thread. Keep that thread alive
for the worker lifetime; its exit kills its workers. Dropping the runtime does
not acknowledge a durable fence. Reconstruct the supervisor and fence before
allowing a successor. External effects already issued cannot be undone by
process termination.

Fencing qualification includes a live child with a deliberately mismatched saved
start time (the PID-reuse case), malformed kernel identity fields, damaged
manifest bindings and partial identities, symlinked evidence, and registry-lock
timeout. The integration test commits a successor while the predecessor remains
unconfirmed and verifies that admission stays blocked. After confirmed stop, the
successor starts; repeating the old fence leaves it running. These tests do not
force kernel PID reuse or certify every possible termination-timeout failure.

## Worker protocol

The `worker` module provides bounded request decoding, proposal encoding and
checkpoint acknowledgement reading. Requests bind scope, generation, definition,
input and optional checkpoint. A checkpoint proposal must wait for the exact
committed checkpoint digest acknowledgement before continuing. A successful exit
alone is not a completed result. The supervisor validates proposals and fences
the worker before accepting terminal state.

Integration tests compile a freestanding static C worker using `cc`; they exercise
results, checkpoint acknowledgement, malformed frames, prohibited syscalls,
CPU enforcement, and reconstruction with a live worker. The memory admission
test verifies that a small anonymous mapping works and an oversized virtual
mapping returns `ENOMEM` under the configured address-space limit. Escape probes
cover process creation, filesystem/network access, signalling, tracing, limit
changes, and reads/writes outside the permitted standard descriptors.
Full linear and graph
sagas replay persisted event bytes with shared and separate databases. A process
kill matrix interrupts their coordinator at ten durable boundaries, including
worker start, task completion and result commit, then reconstructs both logs.
A separate timer matrix kills the coordinator after claim, payload persistence,
inbox acceptance, delivery ACK, and saga decision commit: 20 cases across linear
and graph engines with shared and separate databases. Recovery checks the pending
inbox before redelivery, exact timer bytes, duplicate acceptance after a lost ACK,
and a single committed retry action. These cases stop at retry dispatch; the
orderly timer workflows separately execute the real worker and relay its result.
The qualification report records the completed adversarial campaigns and their
exact revision/scope limits.
