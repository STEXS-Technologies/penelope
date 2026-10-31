# Transaction-local task blob verification reuse — 2026-10-31

Replay now remembers its last verified full blob reference within the current
transaction snapshot. Repeated identical references reuse that verification;
other references are verified normally. Every record is still decoded, hashed
and applied, and quota/head checks remain intact. The external-commit cache
invalidation guard is retained.

Baseline: `05cd3d0124b85d33a796281941850a8169cdbb98`. Candidate: `f583fe9`
(full commit and source hashes in the evidence manifest).

## Safety and scope

The memo is a single local `Option<BlobRef>`: digest, byte count and schema all
participate in equality. It uses constant local storage and adds no heap-backed
collection. It does not survive a replay call, transaction or scope change.
Public callers hold an immediate write transaction while reconstructing, so
another connection cannot modify a verified blob during reuse. References to
older, superseded checkpoints remain validated before being reused.

There is no shortcut for record checksums, semantic application, stored SQL type
and byte bounds on first verification, usage accounting, or head consistency.
Alternating different references miss this one-entry memo. It is deliberately
not a cache of all distinct historical blobs.

The new regression seeds repeated input and checkpoint references, including a
return to the input after a different checkpoint. Nine mutations cover changed
length/schema in repeated references, missing/corrupt input and checkpoint rows,
and a TEXT value. Each must fail without durable changes through append, exact
reservation retry, fencing and maintenance rebuild. Authoritative repair must
permit reservation, append, rebuild and reopen afterward.

A deliberately defective digest-only memo falsely accepts the first changed
length case. Its expected failure is archived as a negative control; production
compares the complete BlobRef. Baseline behavior already passed these invariants;
this change is an optimization, not a new corruption classification.

Twenty isolated new-regression runs, ten per compiler, cover **180 damaged
fixtures and 720 public rejection checks**. Another twenty isolated runs retain
all **960** external-history damage cases and their lock, rollback, repair and
unrelated-scope controls. Zero-byte and 1 MiB reuse fixtures and broader journal
checks run in full qualification.

## Measurement

The primary campaign uses identical candidate benchmark sources against baseline
and candidate production code: 24 alternating AB/BA pairs, **288 processes**.
Each variant runs six processes per pair: ordinary and externally invalidated
checkpoint commits, ordinary and alternating-blob authorization replay, and the
two replay profiles under a separate Rust allocation counter. No agent-owned
build, test or graph job ran during timing. Processes run sequentially on CPU 0,
AMD Ryzen 9 7950X, Linux 7.2.9, release Rust 1.99.

Commit fixtures use public WAL/FULL ports and 128 checkpoint appends with histories
growing from 2 to 129 retained records. External fixtures commit to an unrelated
table from another connection before every timed append; the external transaction
is outside timing, while induced replay stays inside. They cover 0, 32 and 4096
bytes. Uncontended fixtures additionally cover 1 MiB. Record proposal, projection
updates, setup and final validation are outside timing. Encoding, integrity
checks, transaction work and commit remain inside. Final reconstruction and
130-record/one-blob counts must succeed.

Authorization fixtures retry an already reserved start with complete replay of
2, 16 or 128 records, using five-byte blobs. Each has 16 warmups and 250 timed
retries. The repeated profile references the same checkpoint after the input;
the alternating profile switches between two distinct checkpoint blobs. Both
validate final record/reservation counts. At 128 records, blob verification
queries fall from 127 to two for repeated checkpoints, while alternating
references still require all 127. At two records neither profile has a memo hit.
The historical `expected_removed_bytes` field in these fixtures describes an
earlier optimization and is not attributed to this change.

There are **624 ordinary latency observations**, **43,008 timed commits**,
**72,000 timed authorization retries**, and **4,608 separate allocation samples**.
All pairs are retained. Percentages are mean paired `(candidate / baseline - 1)
* 100`, with 20,000 deterministic paired bootstrap resamples and percentile 95%
intervals. Timing columns are arithmetic means, so their ratio need not equal
the mean paired percentage.

### Uncontended checkpoint commits

| Blob bytes | Baseline ns/op | Candidate ns/op | Change | 95% interval |
|---|---:|---:|---:|---:|
| 0 | 27,503.93 | 27,027.62 | -1.2802% | [-4.0019%, +0.4524%] |
| 32 | 26,615.49 | 26,582.11 | -0.0466% | [-1.6621%, +1.4897%] |
| 4,096 | 28,895.64 | 28,620.46 | -0.8866% | [-2.1210%, +0.2557%] |
| 1,048,576 | 480,080.15 | 479,818.90 | -0.0472% | [-0.4799%, +0.2614%] |

### Checkpoint commits after every external write

| Blob bytes | Baseline ns/op | Candidate ns/op | Change | 95% interval |
|---|---:|---:|---:|---:|
| 0 | 301,032.15 | 193,791.19 | -35.3332% | [-36.9895%, -34.3670%] |
| 32 | 297,625.30 | 193,398.05 | -34.8176% | [-36.3609%, -33.8289%] |
| 4,096 | 414,704.04 | 199,242.20 | -51.8089% | [-52.9167%, -51.1306%] |

### Authorization replay with repeated checkpoints

| Records | Baseline ns/op | Candidate ns/op | Change | 95% interval |
|---|---:|---:|---:|---:|
| 2 | 20,988.80 | 20,243.97 | -2.9942% | [-5.8096%, -1.1596%] |
| 16 | 72,322.81 | 48,935.57 | -32.1920% | [-33.5473%, -31.2714%] |
| 128 | 476,398.35 | 259,951.08 | -45.4313% | [-45.6897%, -45.1736%] |

### Authorization replay with alternating checkpoints

| Records | Baseline ns/op | Candidate ns/op | Change | 95% interval |
|---|---:|---:|---:|---:|
| 2 | 20,528.69 | 20,386.43 | -0.6858% | [-1.5856%, +0.4023%] |
| 16 | 72,929.57 | 70,645.35 | -2.8610% | [-5.0085%, -1.3393%] |
| 128 | 483,288.72 | 469,389.57 | -2.8098% | [-3.9125%, -1.9023%] |

Uncontended commit differences are inconclusive. Repeated 16/128-record replay
and all forced-replay commit profiles improve substantially. Small apparent
improvements also occur in profiles with **no memo hits** (two-record ordinary
replay and 16/128-record alternating replay); their mechanism is not established
and they must not be attributed to skipped blob queries. Two-record alternating
replay is inconclusive. All observations and control profiles remain in the data.

All 12 variant/pattern/history allocation profiles have exactly **one allocation,
zero reallocations and 96 cumulatively requested Rust heap bytes** per retry,
both before and after, across 384 samples each. SQLite C allocation, peak or
retained memory are not measured by this counter. The optimization reduces
repeated SQL/verification work; it does not claim an allocation reduction.

## Confirmation and remaining cost of external invalidation

A second campaign compares three identical-benchmark executables: pre-invalidation
code `50856b0`, guarded baseline `05cd3d0`, and the optimized candidate. It has 24
blocks, all six variant orders repeated four times, and alternating placement of
the ordinary/external profile: **144 processes, 504 observations and 64,512 timed
commits**, with all 21 comparisons retained. Analysis uses the same paired method.
No agent-owned jobs ran concurrently with measurement.

The pre-invalidation executable is reused from the prior campaign only after
checking its recorded hash and the unchanged benchmark source hash. The guarded
executable is also hash-identical to the prior campaign's cached-query version.
After both campaigns, rebuilding each source variant reproduces all six primary executable hashes
and the historical pre-invalidation executable hash. All measured source is
restored and checked before commit. The pre-invalidation version is a historical
performance control with the documented corruption bug, not a proposed variant.

### Uncontended confirmation

| Blob bytes | Pre-invalidation ns | Guarded ns | Optimized ns | Optimized vs guarded (95% CI) | Optimized vs pre-invalidation (95% CI) |
|---|---:|---:|---:|---:|---:|
| 0 | 27,426.72 | 28,167.35 | 27,526.24 | -1.9425% [-4.2313%, -0.4905%] | +0.3757% [-0.2103%, +0.9851%] |
| 32 | 26,803.21 | 26,977.69 | 26,801.88 | -0.6352% [-1.2273%, -0.0825%] | +0.0221% [-0.7078%, +0.7445%] |
| 4,096 | 29,042.32 | 29,382.35 | 29,035.26 | -1.1547% [-2.0096%, -0.2838%] | +0.0112% [-0.9001%, +0.8830%] |
| 1,048,576 | 488,959.07 | 488,637.33 | 488,676.71 | +0.0083% [-0.1044%, +0.1260%] | -0.0554% [-0.2617%, +0.1256%] |

### External commit before every append

| Blob bytes | Pre-invalidation ns | Guarded ns | Optimized ns | Optimized vs guarded (95% CI) | Optimized vs pre-invalidation (95% CI) |
|---|---:|---:|---:|---:|---:|
| 0 | 36,517.01 | 297,360.49 | 197,385.09 | -33.6143% [-33.9113%, -33.3107%] | +440.6770% [+436.5951%, +444.5906%] |
| 32 | 36,141.62 | 296,483.18 | 196,460.62 | -33.7292% [-34.0099%, -33.4521%] | +443.6451% [+441.2541%, +446.0010%] |
| 4,096 | 39,385.85 | 417,885.33 | 202,386.40 | -51.5383% [-52.0691%, -51.1555%] | +413.9229% [+411.2763%, +416.4279%] |

Forced-replay improvements reproduce in direction and substantial magnitude;
the two campaigns are reported separately, not pooled or selectively filtered.
The remaining external-invalidation cost is still about 5.1–5.4 times the old
nonvalidating path in these fixtures. Larger or alternating histories and blobs
can cost more; the external profile does not measure 1 MiB blobs or maximum
admitted histories. Record decoding/hashing and semantic application still scan
the history, so this memo does not remove the whole replay cost.

Optimized-versus-pre-invalidation uncontended differences are inconclusive at
all four sizes. Some small uncontended improvements relative to the guarded
version appear only in confirmation; no replay or memo hit occurs inside those
timed appends, so they are not evidence of skipped blob checks. This is consistent
with incidental code-layout or measurement effects, whose cause is unproven.
These are operation-specific measurements, not whole-application overhead or
percentages to add to earlier improvements.

## Qualification and reproduction

Stable full workspace/all targets/all features excluding the separately
configured fuzz package passed. Rust 1.85 SQLite all targets/all features passed:
24 unit, 51 process and 38 task tests plus custom bench executables. Strict
workspace Clippy, formatting, layer boundaries and graph update passed. A test
binding shadow lint was fixed before final Clippy; final targeted tests pass on
both compilers after that naming-only edit. The prior commit passed CI run
41173385862; this report does not claim completed CI for the new commits.

The evidence directory contains all **432 measured processes**, **34 latency
statistics**, allocation samples, source/binary pins, reproduction scripts and
qualification logs. Its verifier checks archive integrity, negative control,
all repeated regressions, raw measurement shape, binary reuse/rebuild provenance
and recomputes both statistical campaigns in temporary storage. See
`docs/verification-data/2026-10-31-task-replay-memo/README.md`.
