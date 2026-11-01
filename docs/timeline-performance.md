# Timeline performance workload

This is the initial reproducible 0.4.0 measurement workload. Inherited release
performance checks retain their own evidence and revision scope.
There is no universal timeline overhead percentage: consumer computation, state
size, history adapter, integrity checks and replay horizon change the cost.

## Workload and comparison boundary

Both implementations compute `state[lane] = 3*state[lane] + input + lane`
modulo 2^64. Complete states contain 1, 128 or 512 u64 lanes (8 bytes, 1 KiB,
4 KiB). Captured u64 inputs have identical values and order. Every profile first
checks both implementations against an independent closed-form oracle; candidate
validation compares the complete expected state. Horizons are 1/8/32/128/512.

The handwritten ring stores 512 inputs, 65 full snapshots and one current state.
It supplies local numeric ordering and revision CAS, with no scope, authority,
finality, intent or retained-branch protocol. Its latency is a lower bound for a
simple local computation, not a complete alternative with equivalent guarantees.
Comparisons therefore disclose absolute costs as well as any ratios.

The public API profiles are:

| Name | Measured work |
| --- | --- |
| `core_prepare` | Validate the borrowed contiguous replay range and exact expected metadata |
| `core_append` | Metadata-only next-input transition |
| `core_reserve_promote` | Prepare, reserve and publish metadata without running application state |
| `history_append` | Admit the final input to a 512-input fixed history, including payload integrity |
| `history_capture` | Full snapshot capture into a 65-anchor store, including integrity verification |
| `history_restore` | Find and verify the genesis snapshot, then copy to supplied scratch |
| `replay` / `replay_yield8` | Advance an already restored session, uninterrupted or with eight-step calls |
| `promote` | Publish a previously replayed and validated candidate |
| `full_correction` | Plan, reserve, restore, replay, validate and publish a state correction |
| `input_correction` | Same full protocol, replacing input 1 with a new immutable revision |
| `competing_corrections` | Replay/validate two candidates, publish one and refuse the stale second |
| `missing_window_refusal` | Refuse a replay range missing its final input |

`full_correction` and replay use a preverified, immutable, indexed input adapter
and an exact complete-state snapshot adapter. They exclude `FixedHistory` scans,
snapshot hashing, authority authentication, serialization, I/O and effect dispatch.
The separate history profiles include their own digest checks. All profiles use
zero staged effects. They do not measure effect-heavy models or promise that a
whole application can allocate nothing.

## Initial evidence, 2026-11-01

[Raw latency/allocation rows, source hashes and host metadata](performance-data/2026-11-01-timeline-baseline/manifest.json)
record an isolated CPU-8 run on Ryzen 9 7950X, Rust 1.99.0, 1,000 samples after
16 warmups. Qualification processes completed before this latency run. Boost,
frequency, OS scheduling and caches were not controlled. This is an initial
same-model comparison, with no baseline-versus-baseline noise qualification or
statistical noninferiority claim.

Full state correction at 512 inputs, nanoseconds per correction:

| State | Penelope median / p95 / p99 | Ring median / p95 / p99 |
| --- | ---: | ---: |
| 8 bytes | 21,291 / 24,507 / 26,691 | 220 / 231 / 231 |
| 1 KiB | 27,974 / 29,507 / 32,272 | 10,350 / 10,451 / 13,065 |
| 4 KiB | 55,006 / 58,062 / 59,114 | 40,097 / 42,733 / 44,245 |

The ring's scalar computation is extremely cheap. Penelope's per-input descriptor
and publication guards dominate that profile. The 4-KiB model spends more time
in the shared consumer computation. These measurements do not establish a small
relative overhead for cheap models or justify applying the old saga workload's
slowdown percentage to timelines. See the raw rows for every horizon and path.

All 301 separately measured operations report zero Rust allocations,
reallocations and requested bytes. For the 8-byte / 1-KiB / 4-KiB states,
respectively, preallocated fixed history occupies 139,112 / 205,152 / 404,832
bytes; live publication occupies 1,672 / 2,688 / 5,760 bytes; each replay session
occupies 1,512 / 2,528 / 5,600 bytes. These counts exclude the fixture's separate
indexed input adapter and are not additive whole-process RSS or stack bounds.

The default fixed history scans configured input/anchor/branch slot bounds; it
does not grow with lifetime history. Its footprint is fully preallocated. State,
snapshot and input buffers need not use this adapter, and snapshots need not be
cloned or hashed every step by the base computation API. COW/delta storage is not
implemented by this workload and has no measured advantage here.

## Capacity matrix and batched throughput

[Raw capacity and throughput records](performance-data/2026-11-01-timeline-capacity/manifest.json)
supplement the original latency run. The 39-row capacity matrix varies input
horizons 8/32/128/512, snapshot intervals 1/8, branch slots 1/2/4, concurrent
sessions 0/2/4, intent slots 0/8/32 and inline state sizes 8/1,024/4,096 bytes.
An input and an effect payload each occupy eight bytes. These rows report
resident capacities even when no payload has been admitted.

For each row, maximum owned domain storage in the declared inline profile is:
`history_bytes + live_bytes + sessions * session_bytes`. Live and replay session
sizes already include their states and bounded effect buffers; do not add those
again. History includes all anchor/input/branch slots. Separate descriptors,
caller input cursors, construction temporaries, application buffers and foreign
payload allocations are outside that sum. For example, the 512-input/65-anchor,
four-branch/four-session, 4-KiB-state profile occupies 444,800 bytes with eight
intents per owner and 473,600 with 32. Snapshot intervals describe provisioned
anchor capacity, not an automatic capture/eviction policy.

Throughput uses one timer around each 1,000-operation batch, separately for pure
plan preparation and full state correction. Full correction includes creation
of a fresh live owner. Rows contain path, state words, steps, operations and
elapsed nanoseconds; operations/second is `operations * 1e9 / elapsed_ns`.
Both use the same prebuilt captured-input fixture. These CPU-8 batches ran while
other qualification processes were active, so their rates are development
observations rather than isolated throughput guarantees or regression thresholds.

```sh
cargo bench -p penelope-timeline --bench timeline_memory --locked
taskset -c 8 cargo bench -p penelope-timeline --bench timeline_throughput --locked
```

## Sampled hosted stack footprint

The [compressed Massif records and executable/tool metadata](performance-data/2026-11-01-timeline-memory/manifest.json)
retain 749 snapshots of the complete hosted allocation benchmark at `c88f704`.
Valgrind 3.27.1 with stack accounting sampled a maximum of 2,579,672 stack bytes
and 2,581,280 combined heap/allocator-overhead/stack bytes. A separate explicit
64-KiB stack positive control measured 66,168 stack bytes and passed the runner's
minimum-size assertion.

These numbers include benchmark fixture construction, repeated by-value history
initialization/temporaries, all three state sizes, every horizon and reporting.
They do not attribute stack to one timeline operation, bound arbitrary consumer
callbacks, include all compiler red-zone usage or establish a portable worst-case
call-chain bound. Sampling may miss transient peaks. The caller-owned scratch
state itself occupies 8 / 1,024 / 4,096 bytes in these profiles. Large inline fixed
histories require deliberate placement and sufficient construction stack; their
zero-allocation property does not make them suitable for a small embedded stack.
Use a different storage adapter or application-owned static/heap placement when
that profile requires it, and qualify the actual target and compiler.

Reproduce the complete measurement (requires native Valgrind and Rust):

```sh
python3 scripts/measure-timeline-memory.py target/timeline-memory-new
```

The output directory must not already exist. The runner identifies the exact
Cargo executable, rejects custom compiler settings, verifies the positive
control, and fails on incomplete memory records. It does not apply a universal
stack ceiling to a generic user model.

## Reproduction

```sh
PENELOPE_TIMELINE_SAMPLES=1000 taskset -c 8 \
  cargo bench -p penelope-timeline --bench timeline_operations --locked
cargo bench -p penelope-timeline --bench timeline_allocations --locked
```

Choose an allowed CPU on the measurement host. Both commands use Cargo's release
benchmark profile. The latency executable has no counting allocator. It performs
16 warmups and then records individual operation durations, with fixture setup,
allocation of timing samples, sorting and printing outside the timer. Operations
mutate their prepared fixture through a reference; whole history/ring copies are
not charged as operation arguments. Candidate ownership moves that are part of
the public promotion protocol remain included.

Each `latency` CSV row contains name, state words, steps, samples, median ns,
p95 ns and p99 ns. Percentiles use sorted nearest ranks. `timer_control` measures
the timer/black-box floor; it is not subtracted. Tiny operations near that floor
do not support precise relative comparisons. OS scheduling, caches and frequency
remain uncontrolled; sample percentiles describe this run, not a production SLA.

The separate allocation executable counts Rust allocator allocation, reallocation
and requested bytes around the same operations and asserts all three are zero.
Fixture creation, timing samples and reporting are excluded. It does not intercept
foreign allocators or measure process RSS. Each `allocation` CSV row contains
name, state words, steps and the three counters.

Each `memory` CSV row contains state words and `size_of` bytes for fixed history,
live publication owner, replay session, handwritten ring and one scratch state.
The fixed history has 512 input slots, 65 anchor slots and two branch slots;
live publication has two session reservations and no effect slots. These are
resident object sizes, not a stack high-water measurement. Application input
payloads are inline here; pointer-owning consumer types must account for their
separate allocations. General maximum storage is the sum of configured slot
metadata, admitted payload charges, owned live/scratch states, candidate sessions
and staged effects. Pins and retained ancestry can prevent eviction before a
chosen floor; capacity exhaustion refuses admission rather than silently growing.
