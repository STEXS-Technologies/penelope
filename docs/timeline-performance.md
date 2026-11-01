# Timeline performance workload

This document records the reproducible 0.4.0 workload and its successive audits.
Inherited release performance checks retain their own evidence and revision scope.
There is no universal timeline overhead percentage: consumer computation, state
size, history adapter, integrity checks and replay horizon change the cost.

The latest [state-width qualification](#snapshot-state-width-qualification) covers
640 profiles. Earlier sections preserve their original measurements and source
scope; their figures are not all measurements of the current implementation.

## Workload and comparison boundary

Both implementations compute `state[lane] = 3*state[lane] + input + lane`
modulo 2^64. Complete states contain 1, 128 or 512 u64 lanes (8 bytes, 1 KiB,
4 KiB). Captured u64 inputs have identical values and order. Each initial comparison
profile checks both implementations against an independent closed-form oracle;
candidate validation compares the complete expected state. Horizons are
1/8/32/128/512.

The handwritten ring stores 512 inputs, 65 full snapshots and one current state.
It supplies local numeric ordering and revision CAS, with no scope, authority,
finality, intent or retained-branch protocol. Its latency is a lower bound for a
simple local computation, not a complete alternative with equivalent guarantees.
Comparisons therefore disclose absolute costs as well as any ratios.

The workload's public API profiles include:

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

Later sections add retained-history, snapshot selection/materialization and
effect-buffer profiles, each with its own archived comparison boundary.

`full_correction` and replay use a preverified, immutable, indexed input adapter
and an exact complete-state snapshot adapter. They exclude `FixedHistory` scans,
snapshot hashing, authority authentication, serialization, I/O and effect dispatch.
The separate history profiles include their own digest checks. Initial profiles
use zero staged effects; later effect-storage audits below record their own
workloads and limits. The initial comparison does not measure effect-heavy models
or promise that a whole application can allocate nothing.

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

## Effect-buffer audit

[Matched baseline/optimized measurements and source hashes](performance-data/2026-11-01-effects-audit/manifest.json)
compare `a524274` with the audited implementation using identical expanded
benchmarks and separate Cargo target directories. Each version has three CPU-8
runs, 1,000 samples per profile after 16 warmups. Values below are the median of
three run medians; reductions describe this synthetic workload, not application
slowdown or a statistical guarantee.

The 84 additional profiles exercise capacities 8/64/512 and pending counts
0/1/8/64/512 where they fit. They measure inspection, staging, append, zero-replay
correction that retains/discards intents, and one/all eligible handoffs. State and
payloads are u64. The in-memory handoff adapter checks payload integrity and
logical-step/identity ordering; database, network and external execution are
excluded. Fixture construction is outside latency samples. A full-buffer append
profile measures capacity refusal. The earlier 391 profiles retain their
individual measurement boundaries.

Capacity 512, nanoseconds per operation:

| Operation | Pending intents | Before | After | Reduction |
| --- | ---: | ---: | ---: | ---: |
| Inspect intents | 8 | 220 | 20 | 90.909% |
| Stage complete step | 64 | 25,790 | 12,955 | 49.767% |
| Append one intent | 64 | 13,516 | 6,052 | 55.223% |
| Correct while retaining intents | 512 | 75,236 | 21,842 | 70.969% |
| Handoff all | 1 | 2,155 | 101 | 95.313% |
| Handoff all | 8 | 15,260 | 912 | 94.024% |
| Handoff all | 64 | 135,682 | 9,338 | 93.118% |
| Handoff all | 512 | 2,096,603 | 88,551 | 95.776% |

Occupied intents now form a dense prefix. Small handoffs scan only occupied
slots; batches with at least eight eligible intents and an eight-intent budget
sort metadata once and remove from the tail. Stable dispatch order remains step,
then identity. Diagnostic iteration has unspecified storage order. Inline buffer
initialization and movement still depend on configured capacity; this does not
make every path proportional only to pending intents.

All 475 separately instrumented operations record zero Rust heap allocations,
reallocations and requested bytes. Consumer callbacks remain outside that
promise. All 39 capacity-profile rows are identical before/after. Full-capacity
inspection measured 241 to 251 ns (4.149% more); very short operations approach
the timer floor. No unchanged non-effect profile with baseline median at least
500 ns changed by more than 20%. Scheduling, frequency and caches were not
controlled; all rows, including unfavorable ones, remain in the archive.

Matched Massif runs cover the entire expanded allocation benchmark, including
fixtures/reporting. Both pass the 64-KiB stack positive control. Maximum sampled
stack is 2,708,560 bytes before and 2,708,592 after; sampled total is 2,710,168 and
2,710,200 bytes. These are whole-process sampled peaks, not individual operation
costs, portable call-stack limits or a comparison to the older smaller workload.
The successful runs use workspace-local `TMPDIR` and `VALGRIND_OPTS=--vgdb=no`
after an initial host temporary-directory quota failure.

## FixedHistory lookup follow-up

[Matched measurements, binary/source hashes and all 520 profiles](performance-data/2026-11-01-history-lookup/manifest.json)
compare `3de62ef` with checked input slot hints and a step-filtered exact fallback.
Canonical ancestry resolution verifies its selected payload directly instead of
searching for the same entry again. Every read still checks payload digest and
declared charge; the hint must match the full requested descriptor. Imports,
eviction, slot reuse and branch-local records may miss the hint and use scans.
No new index fields or dependencies are added.

The 45 new profiles measure read-all, descriptor collection and full correction
through `FixedHistory`, using u64 state/inputs and no intents. Layouts are a root
branch, a child inheriting parent input aliases, and a history initially holding
512 inputs whose floor advances to 512-N with a complete floor snapshot. The
512-input eviction case retains floor zero as a control. Fixture construction is
outside timing. Full correction includes descriptor collection/scratch copy,
planning, reservation, history plan checks, verified snapshot restore, batched
pinned replay, validation and publication. It excludes new history admission,
forking, snapshot capture and external I/O. These boundaries differ from the
captured/indexed adapter profiles, so their absolute times are not interchangeable.

Median of three run medians, nanoseconds per root-branch full correction:

| Replayed inputs | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| 1 | 2,105 | 2,115 | -0.475% |
| 8 | 3,747 | 3,517 | 6.138% |
| 32 | 12,214 | 8,356 | 31.587% |
| 128 | 93,861 | 27,623 | 70.570% |
| 512 | 1,275,926 | 106,195 | 91.677% |

At 512 inputs, root read-all falls from 341,650 to 29,347 ns (91.410%) and
collection from 444,799 to 27,924 ns (93.722%). Inherited full correction falls
from 2,535,701 to 735,451 ns (70.996%); ancestry and exact imported-version
precedence still require fallback work. Longer retained windows benefit most;
small operations are dominated by fixed checks and measurement noise.

All 520 separately instrumented operations allocate zero Rust heap bytes. The
39 capacity-profile rows remain available; these describe object sizes, not
per-operation stack use. No new sampled-stack claim follows from this expanded
benchmark. Both versions use identical benchmark source and separate build
directories, three CPU-8 runs and 1,000 samples after 16 warmups. Frequency,
caches and scheduling remain uncontrolled; the raw archive includes unfavorable
rows and does not establish a universal application overhead percentage.

## Inherited-prefix scan follow-up

[Alternating matched measurements and source/binary hashes](performance-data/2026-11-01-ancestry-scan/manifest.json)
compare `a2afaad` with a narrower canonical-input search. A child inherits its
parent's admitted input prefix and can append canonical inputs only after that
fork point. Within the prefix, resolution now skips the impossible full scan of
child-local canonical inputs and follows an explicit replacement or parent.
The checked slot hint remains available. Materialized roots and inputs appended
after the fork point retain local lookup; exact stored imports retain precedence.
Snapshot resolution is unchanged: child snapshots can cover inherited steps.

The benchmark source and all 520 profiles are unchanged. The inherited fixture
uses one child and scalar u64 state/inputs; deeper chains are tested for selection
correctness but not timed here. Full correction retains the previously documented
collection, plan, reservation, history verification, restore, replay, validation
and publication boundaries. Admission, fork creation, fixture construction and
external I/O remain excluded.

Median of three run medians, nanoseconds per inherited full correction:

| Replayed inputs | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| 1 | 3,006 | 2,485 | 17.332% |
| 8 | 8,076 | 5,160 | 36.107% |
| 32 | 26,151 | 14,478 | 44.637% |
| 128 | 122,958 | 57,100 | 53.561% |
| 512 | 726,996 | 270,753 | 62.757% |

At 512 inherited inputs, collection falls from 177,924 to 27,022 ns (84.813%)
and read-all from 344,906 to 189,837 ns (44.960%). Exact imported-record lookup
can still scan configured slots. Root full correction measures 103,480 to
102,939 ns; this does not establish a meaningful improvement on that control.

An unfavorable sparse control remains visible: eight-input evicted read-all
measures 1,563 to 2,023 ns (29.431% more). Its run medians overlap widely:
1,553–2,064 ns before and 1,513–2,033 ns after. The corresponding complete
correction measures 6,593 to 6,432 ns. These observations do not establish that
every path improves or a universal application slowdown percentage.

Runs alternate before/after in the archived order, use CPU 8, 1,000 samples and
16 warmups, and run after compilation/qualification activity has stopped.
Separate build directories and equal-length executable paths retain distinct
binary hashes. Frequency, scheduling, caches and ASLR remain uncontrolled;
run ranges are descriptive, not confidence intervals. The initial candidate's
collection regressions led to guarding only the missing-hint scan fallback.
All final rows, including unfavorable movements, remain archived.

All 520 separately instrumented operations allocate zero Rust heap bytes. All
39 capacity rows match the preceding baseline archive; no public fields or
index storage were added. This round makes no new sampled-stack claim.

## Snapshot lookup and materialization follow-up

[Matched measurements and same-binary controls](performance-data/2026-11-01-snapshot-lookup/manifest.json)
compare `843e333` with step-filtered exact snapshot lookup and direct verification
of a canonical match. Resolution no longer searches again for an entry it already
selected. Complete descriptor, digest and declared-charge checks remain required;
exact correction imports retain their precedence. Child-local snapshots inside an
inherited input prefix remain eligible for canonical selection.

The 40 added profiles bring the workload to 560. They use 512 retained inputs,
u64 state/payloads, zero intents and 65 snapshot slots. For these profile names,
the count column means initial canonical snapshots (1/8/32/63), spaced at steps
0, 8, 16 and so on. Target lookup selects the final canonical snapshot. Layouts
cover a root, a child corrected at step 512, a child-local target snapshot inside
that inherited prefix, and an explicit root correction import. In the imported
layout, canonical lookup keeps the original while restore selects the import.
Fixtures, snapshot admission and forks are outside timing. Materialization includes
preflight, cloning and admission into a detached store, excluding caller replacement,
disposal of superseded history, persistence and I/O. Scalar Clone costs are not
representative of arbitrary consumer state.

At 63 canonical snapshots, median of three run medians, nanoseconds:

| Operation | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| Root canonical snapshot lookup | 250 | 120 | 52.000% |
| Root restore | 240 | 120 | 50.000% |
| Inherited canonical lookup | 271 | 140 | 48.339% |
| Inherited restore | 411 | 170 | 58.637% |
| Child-local canonical lookup | 241 | 120 | 50.207% |
| Root materialization | 176,240 | 164,337 | 6.754% |
| Inherited materialization | 204,435 | 190,268 | 6.930% |

Unfavorable input controls remain in the report: eight-input inherited read-all
measures 1,734 to 2,214 ns, while evicted collection measures 1,363 to 1,693 ns
at eight inputs and 5,160 to 6,583 ns at 32. Six additional runs use the identical
baseline executable on both sides of the self-comparison. They also show control
variation: inherited read-all(8) spans 1,644–2,184 ns, and root snapshot lookup(63)
spans 241–381 ns. The latter remains distinct from all three optimized medians
of 120 ns. Variability does not prove every unfavorable result is noise, and
these measurements do not establish an improvement on every path.

Final version comparisons alternate baseline/optimized runs; the supplemental
self-comparison uses the baseline binary in every process. All use CPU 8, 1,000
samples after 16 warmups, separate version build directories, and no concurrent
qualification builds/tests. Frequency, scheduling, caches and ASLR remain
uncontrolled. Run ranges and maximum run p99 values are descriptive, not confidence
intervals. The archive retains all raw rows, source/binary hashes and a patch for
reproducing the expanded benchmark on the baseline production revision.

All 560 separately instrumented operations record zero Rust heap allocations,
reallocations and requested bytes. All 39 capacity rows match the preceding
archive. This round adds no index fields and makes no new stack-bound claim.

## Detached materialization follow-up

[The materialization archive](performance-data/2026-11-01-materialization/manifest.json)
compares the snapshot-lookup baseline `2e83d59` with direct construction of a
detached history. A fresh destination has one root and its floor snapshot;
materialization fills the remaining empty slots in logical-step order instead of
repeatedly searching for free slots and duplicates through public admission.
This removes destination searches; source resolution still depends on configured
history and lineage bounds. Complete source preflight remains before any clone.
Every actual clone still receives digest, per-payload and checked aggregate-charge
validation. The destination publishes its admitted frontier only after construction.

The unchanged 560-profile workload includes eight materialization profiles:
512 retained scalar inputs, 1/8/32/63 initial canonical snapshots, root or inherited
branch. Their median reductions range from 21.190% to 25.952%. At 63 snapshots:

| Operation | Before, ns | After, ns | Reduction |
| --- | ---: | ---: | ---: |
| Root materialization | 167,203 | 123,810 | 25.952% |
| Inherited materialization | 192,213 | 148,147 | 22.926% |

These are medians of three run medians, with 1,000 samples after 16 warmups per
profile per run, pinned to CPU 8. Separate build directories and equal-length
executable paths isolate the versions. Six runs alternate versions before
qualification builds/tests start. The archive includes all raw rows, run ranges,
maximum run p99 values and source/binary hashes. Frequency, scheduling, caches,
ASLR and other host processes remain uncontrolled. There is no fresh same-binary
control in this round; prior controls retain their earlier source scope.

Some unchanged controls move unfavorably: one-input evicted read-all measures
210 to 271 ns, and scalar one-input core append measures 40 to 50 ns. Retaining
these results avoids claiming that every path improved or that variability
explains every unfavorable movement. This is a materialization improvement,
not a measured whole-game speedup or universal library overhead percentage.

Materialization includes preflight, cloning and detached construction, excluding
fixture creation, caller replacement, disposal of superseded stores, persistence
and I/O. Arbitrary consumer Clone cost can dominate these scalar workloads.
All 560 separately instrumented operations record zero Rust heap allocations,
reallocations and requested bytes; consumer clones may allocate. All 39 capacity
rows match the preceding archive. No fields or indexes are added, and this pass
makes no new sampled-stack or worst-case call-stack claim.

## Snapshot state-width qualification

[The state-width archive](performance-data/2026-11-02-snapshot-width/manifest.json)
extends snapshot lookup, restore and materialization to 8-byte, 1 KiB and 4 KiB
states, bringing the shared workload to 640 profiles. Production source remains
`735f809`; this follow-up checks the existing materialization optimization against
baseline `2e83d59` using identical expanded benchmark source in both builds.

All 24 materialization profile medians improve, by 11.743–28.308%. At 512 retained
inputs and 63 initial canonical snapshots, median of three run medians:

| State | Materialization | Before, ns | After, ns | Reduction |
| --- | --- | ---: | ---: | ---: |
| 1 KiB | Root | 254,713 | 199,947 | 21.501% |
| 1 KiB | Inherited | 289,239 | 227,219 | 21.442% |
| 4 KiB | Root | 514,234 | 448,758 | 12.733% |
| 4 KiB | Inherited | 542,980 | 479,217 | 11.743% |

The smaller relative gain for larger state limits how the scalar results can be
generalized. These are full detached-store rebuilds when the caller invokes
`materialize_branch`; the library does not automatically rebuild history on every
replay step. The synthetic integer model does not measure whole-game frame cost
or arbitrary consumer Clone and integrity-hash implementations.

Each width uses the same 512 scalar inputs, 65 snapshot slots, snapshot spacing
and branch/import layouts as the preceding audit. Outside timing, the fixtures
check the complete restored state, selected canonical descriptor and all 512
materialized input values. The added checks run against both production versions.
Timed regions retain the earlier assertions and exclude fixture creation, forks,
caller replacement, disposal of superseded stores, persistence and I/O.

Six alternating version runs use 1,000 samples after 16 warmups per profile,
CPU 8, separate build directories and equal-length executable paths. Compilation
and qualification tests start after timing completes. All raw rows, run ranges,
maximum run p99 values, source/binary hashes and a baseline benchmark patch are
retained. Frequency, scheduling, caches, ASLR and unrelated host activity remain
uncontrolled. No fresh same-binary control was run for this source comparison.

Unchanged controls include unfavorable movements: evicted read-all(8) measures
1,613 to 2,044 ns, while scalar core append(1) measures 40 to 50 ns. These results
do not establish improvement on every path or statistical noninferiority. All
640 separately instrumented operations record zero Rust heap allocations,
reallocations and requested bytes; arbitrary consumer clones may allocate. All
39 capacity rows are unchanged. This pass adds no library fields and provides no
new sampled-stack measurement or worst-case call-stack bound.
