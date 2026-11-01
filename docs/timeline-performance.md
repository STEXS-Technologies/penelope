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
| `owned_apply_input` | Derive/hash one input record, clone live state, compute, validate and publish without history admission |
| `core_reserve_promote` | Prepare, reserve and publish metadata without running application state |
| `history_append` | Admit the final input to a 512-input fixed history, including payload integrity |
| `history_capture` | Full snapshot capture into a 65-anchor store, including integrity verification |
| `history_restore` | Find and verify the genesis snapshot, then copy to supplied scratch |
| `replay` / `replay_yield8` | Advance an already restored session, uninterrupted or with eight-step calls |
| `promote` | Publish a previously replayed and validated candidate |
| `full_correction` | Plan, reserve, restore, replay, validate and publish a state correction |
| `scoped_full_correction` | The same correction through the scoped convenience handle, including automatic reservation lifecycle |
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
Cargo executable, rejects explicit Rust-flag/wrapper overrides, verifies the positive
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

## Convenience API follow-up

[Raw follow-up records and source hashes](performance-data/2026-11-01-timeline-devex/manifest.json)
measure `453039f` on the same host/compiler, with 1,000 samples after 16 warmups,
CPU-8 affinity and a separate allocation executable. The full SQLite fuzz
campaign was running concurrently; host frequency, scheduling and caches were
uncontrolled. Compare the scoped and low-level APIs within this run. Differences
from the initial binary are not evidence of a general speed improvement.

| State | Owned input median / p95 / p99, ns | Scoped 512-step correction median, ns | Low-level 512-step correction median, ns |
| --- | ---: | ---: | ---: |
| 8 bytes | 140 / 151 / 151 | 15,641 | 15,530 |
| 1 KiB | 211 / 221 / 231 | 24,698 | 24,307 |
| 4 KiB | 331 / 350 / 361 | 55,548 | 55,637 |

Owned input here is one new step after a 511-input head. It includes descriptor
hashing, complete isolated state cloning, computation, validation and publication;
fixture preparation and history admission/capture are excluded. Scoped correction
includes planning, reservation, restore, replay, validation and publication. Both
correction paths use the same preverified input/snapshot adapter and stage no
effects. All 331 separately counted profiles, including the 30 new convenience
profiles, recorded zero allocations/reallocations/requested bytes. Consumer
`Clone`, digest and model implementations can have different allocation behavior.
These samples do not establish statistical noninferiority or a universal overhead.

The [repeated sampled stack profile](performance-data/2026-11-01-timeline-devex/memory/manifest.json)
includes the convenience profiles and large fixture construction. Across 953
Massif samples, maximum observed stack usage was 2,585,400 bytes and maximum total
2,587,008 bytes. The separate 64-KiB positive control recorded 66,168 stack bytes.
This remains a whole hosted benchmark observation, including construction
transients, rather than a per-operation or portable worst-case stack bound.

## Proposed-head validation audit

[Source-83a51c6 records](performance-data/2026-11-01-timeline-validation/manifest.json)
repeat the workload after correction validators were aligned with the proposed
publication head. The retained run started after other local verification/build
jobs completed; the full SQLite fuzz campaign was still running. The same host,
compiler, CPU-8 affinity, 1,000 samples and 16 warmups apply. The earlier preview,
which overlapped compilation/workspace tests/coverage and showed large outliers,
is retained separately as `loaded-latency.csv`. Host frequency, scheduling and
caches remain uncontrolled; this is not a statistical overhead comparison.

| State | Owned input median / p95 / p99, ns | Scoped 512-step correction median, ns | Low-level 512-step correction median, ns |
| --- | ---: | ---: | ---: |
| 8 bytes | 131 / 141 / 141 | 17,924 | 18,115 |
| 1 KiB | 200 / 201 / 211 | 26,452 | 26,310 |
| 4 KiB | 341 / 351 / 351 | 55,487 | 54,445 |

The same adapter, zero-effect and setup-exclusion boundaries above apply. All
331 allocation profiles again record zero allocations/reallocations/requested
bytes. The repeated whole-benchmark Massif profile records 951 samples, maximum
observed stack of 2,585,416 bytes and total of 2,587,024 bytes; the separate positive
control records 66,168 stack bytes. These samples include fixture construction,
do not bound arbitrary consumer callbacks and are not portable worst-case bounds.

## Client prediction speed audit before optimization

[Three repeated runs and the allocation audit](performance-data/2026-11-01-client-prediction/manifest.json)
cover the pre-optimization production sources at `11317c2` (unchanged from
`83a51c6`). Each run uses
1,000 samples after 16 warmups, CPU-8 affinity, the same host/compiler as above,
and the same integer workload. Other host processes remained active; frequency,
caches and scheduling are uncontrolled. Values below are ranges of the three
run medians, in microseconds, rather than confidence intervals.

| Complete state | One owned input at head 511 | 8-tick correction, indexed / captured | 32-tick correction, indexed / captured | 512-tick correction, indexed / captured |
| --- | ---: | ---: | ---: | ---: |
| 8 bytes | 0.130 | 0.761–0.781 / 1.193–1.213 | 1.914–1.954 / 4.418–4.488 | 25.129–25.209 / 331.150–342.262 |
| 1 KiB | 0.171–0.181 | 1.002–1.042 / 1.412–1.453 | 2.656–2.725 / 5.040–5.140 | 35.678–36.471 / 343.865–346.901 |
| 4 KiB | 0.330–0.401 | 1.863–2.024 / 2.264–2.465 | 4.979–5.400 / 7.384–7.936 | 67.260–73.072 / 377.089–384.884 |

Every correction column includes planning, reservation, restore, replay with
one-tick chunks, validation and publication. Chunks run consecutively, so this
measures total work rather than elapsed frames. Each sample validates its result
against the independent oracle. Raw CSVs retain p95/p99: the largest of the three
p99 values for 512-tick indexed replay is 31.341/41.040/77.410 microseconds for the
three state sizes; captured replay is 465.610/361.699/397.458 microseconds.

The simple `CapturedInput` array performs a linear descriptor search per read
and rechecks the input digest. Across an N-input prefix this gives quadratic
lookup work. The indexed fixture checks the exact descriptor in constant time
and trusts its immutable, preverified payload. Both use the same preverified
snapshot fixture, so these differences include input hashing and lookup, rather
than an isolated search comparison. The array contains 512 entries; a suffix
near its end can scan a longer prefix than the cases measured here. This is a
real scaling limitation of the convenience adapter at that revision. The
optimization below addresses contiguous buffers. A prediction integration
with long retained windows can implement an indexed `InputStore` while retaining
exact descriptor/content integrity checks. The public API already permits that.

All 376 separately instrumented profiles, including the 45 new prediction
profiles, recorded zero Rust allocations/reallocations/requested bytes during
the measured operation. Fixture construction and captured-array admission are
outside timing and counting. Input history admission/capture, effects, snapshot
admission/integrity hashing, networking, game physics, rendering and actual
frame scheduling are excluded. These results support low measured substrate
cost in this synthetic workload; they do not establish a universal game slowdown
percentage or a frame-time guarantee.

## Captured-input lookup optimization

[Matched before/after records](performance-data/2026-11-01-captured-input-optimization/manifest.json)
compare `11317c2` with the optimized captured-input lookup, using identical
benchmarks and separate Cargo build directories. Distinct executable hashes are
retained. Each version has three runs of 1,000 samples per profile after 16
warmups, pinned to CPU 8. Values below are the median of the three run medians.
Baseline runs precede optimized runs; host load, frequency and scheduling remain
uncontrolled. These are workload-specific observations, not confidence intervals.

| State | Full 512-tick correction before → after, µs | Time reduction | Final 32 ticks of a retained 512-input buffer before → after, µs | Time reduction |
| --- | ---: | ---: | ---: | ---: |
| 8 bytes | 337.122 → 56.619 | 83.21% | 38.194 → 3.998 | 89.53% |
| 1 KiB | 351.309 → 63.363 | 81.96% | 39.256 → 4.899 | 87.52% |
| 4 KiB | 381.628 → 95.465 | 74.98% | 42.703 → 8.386 | 80.36% |

Both columns include planning, reservation, restore, replay in consecutive
one-tick chunks, validation and publication. Suffix profiles additionally verify
the captured snapshot digest on restore, identically before and after. Each
sample checks the final state against the independent oracle. Raw records retain
all 391 profiles and their p95/p99 values, including unchanged control paths and
small-window results. Small prefixes show little benefit: eight-input prefix
corrections have run-median aggregates of 1.222 → 1.263, 1.493 → 1.453 and
2.304 → 2.384 microseconds for the three state sizes. This is not an across-the-board
speedup claim. Normal owned-input application does not use this lookup.

Arrays and slices now derive a possible slot from the requested step minus the
first retained step. The candidate must match the entire descriptor; every read
still checks the selected payload digest. Missing or mismatched candidates fall
back to a step-filtered exact-record scan, so unordered, gapped, mixed-branch and
multiple-version buffers remain supported. For contiguous prediction buffers,
lookup is constant-time per step and linear over a complete replay. Arbitrary
layouts retain a linear worst-case lookup. No sorting, allocation, additional
index, constructor or application API change is required.

All 391 separately counted optimized profiles record zero Rust allocations,
reallocations and requested bytes in the measured operation. The synthetic
integer workload, setup/history-admission exclusions, zero-effect profile and
consumer-callback limitations of the preceding audit still apply. Transport,
game physics, rendering and actual frame scheduling are not measured here.
