# External commits invalidate task projection caches — 2026-10-31

An append now revalidates its task history after another SQLite connection has
committed, even if the derived head still matches the cached projection. The old
cache could authorize appends over missing input or superseded checkpoint blobs,
missing interior records, corrupt earlier bytes/checksums, or unexplained quota
usage. Reusing a deleted input blob could silently restore it and charge its
bytes again. Cold reconstruction already rejected these cases.

Baseline: `50856b001f81978daea5044947557a26429d9740`. Candidate: `f0c68ce`
(full commit and source hashes in the evidence manifest).

## Mechanism and limits

The cached projection additionally retains `PRAGMA main.data_version`. Commit
samples it after acquiring `BEGIN IMMEDIATE`, so another writer cannot commit
between the sample, reconstruction and append. A mismatch forces the existing
bounded, validating reconstruction and head/usage comparison before any writes.
Successful commit and maintenance rebuild cache the version sampled inside their
write transaction. A failed attempt leaves the previous cache revision intact.
The public API, schema, admission bounds and WAL/FULL policy are unchanged.

SQLite documents that [data_version changes for commits on other connections,
including connections in other processes, and not for this connection's own
commits](https://www.sqlite.org/pragma.html#pragma_data_version). Values are only
compared on the same connection. Uncontended append retains its existing cached
projection fast path and adds one prepared counter query. Scope and head checks
remain in place. A database-wide counter necessarily invalidates the scope cache
for unrelated external writes too; it is not a per-scope change detector.

This is validation after external SQLite commits, not detection of raw disk
modification without a SQLite commit, authenticated history against a malicious
rewriter, or a guarantee about arbitrary triggers modifying old history during
this connection's own writes. Public record reads retain their selected-record
and tail validation contract; this change concerns append authorization.

## Regression evidence

The regression seeds input, start and two checkpoints, then changes durable rows
from another connection without changing the head. It tests eight damages, both
new-output and reused-input appends, and warm, explicitly rebuilt and cold
caches: **48 cases**. Damages cover missing input, missing superseded checkpoint,
same-length corrupt versions of those blobs, missing interior record, bad earlier
checksum, bad earlier encoded record and an unexplained extra blob.

The baseline falsely acknowledges and changes durable rows in **30 cases**.
The final code returns Corrupt and leaves all five table snapshots unchanged in
all 48. Authoritative repair permits the same journal to append; reconstruction
and reopen then succeed. The missing-input/reused-blob case is rejected before
reinsertion, preventing the double charge.

Additional public-port controls hold an external deleting write transaction:
with a zero busy timeout the journal returns Unavailable without changing rows;
after rollback it succeeds, while after commit it returns Corrupt until repair.
Another control damages an unrelated scope, then creates and drops an index;
the healthy scope continues to append while reconstruction still rejects the
damaged one.

Twenty isolated runs, ten on Rust 1.99 and ten on Rust 1.85, exercise **960 damage
cases**, 40 lock/commit-or-rollback fixtures and 20 unrelated-scope/schema-change
fixtures. Logs retain all case observations rather than only summary counts.

## Performance tradeoff

The public WAL/FULL `task_blob_commits` benchmark compares baseline, uncached
counter-query and cached counter-query implementations. The final implementation
caches the query. Each of 24 blocks uses one of all six variant orders, repeated
four times, and both profiles alternate their position between blocks. Executables
run sequentially, pinned to CPU 0 on an AMD Ryzen 9 7950X, Linux 7.2.9, release Rust
1.99. No agent-owned build, test or graph jobs ran during timing.

Each size fixture commits an input and start, then times 128 checkpoint commits
reusing the input. Proposal, projection updates, setup and final reconstruction
are outside timing. Encoding, integrity checks, transaction work and WAL/FULL
commit are inside. Final record/blob counts and reconstruction must succeed.

The added external-commit profile inserts into an unrelated table from another
connection before every timed append. Those external transactions are outside
timing; their effect on the journal's cache remains inside. Histories grow from
2 to 129 retained records. This deliberately forces replay on every append, and
only measures 0, 32 and 4096-byte blobs. No external-write result is claimed for
1 MiB blobs or for the maximum admitted history.

There are **144 processes, 504 observations and 64,512 timed commits**. All blocks
are retained. Percentages below are mean paired `(after / before - 1) * 100`,
with 20,000 deterministic paired bootstrap resamples and percentile 95% intervals.
Timing columns are arithmetic means, so their ratio need not equal the paired
percentage. The full archive contains all **21** variant comparisons.

Final cached counter query versus baseline, without intervening external writes:

| Blob | Baseline ns/append | Final ns/append | Change | 95% interval |
|---|---:|---:|---:|---:|
| 0 bytes | 27,596.5 | 27,610.1 | +0.1770% | [−1.3431%, +1.5961%] |
| 32 bytes | 27,287.6 | 27,194.8 | +0.2229% | [−2.7299%, +2.3900%] |
| 4 KiB | 29,113.1 | 29,368.1 | +0.9240% | [−0.2276%, +2.1218%] |
| 1 MiB | 485,105.0 | 484,766.1 | −0.0607% | [−0.5088%, +0.3312%] |

All four differences are inconclusive; this does not prove zero overhead. The
uncached candidate regresses 4 KiB appends by +2.5832% [1.2099%, 4.2303%]; caching
improves that candidate by −1.5305% [−2.9957%, −0.2338%]. Other cached/uncached
comparisons, including the external-commit profiles, are inconclusive.

With an unrelated external commit before **every** append:

| Blob | Baseline ns/append | Final ns/append | Change | 95% interval |
|---|---:|---:|---:|---:|
| 0 bytes | 37,723.4 | 310,888.2 | +728.5997% | [+701.2089%, +759.1087%] |
| 32 bytes | 36,953.2 | 305,706.3 | +727.1130% | [+721.4419%, +731.7351%] |
| 4 KiB | 40,360.0 | 425,205.0 | +955.0124% | [+946.2632%, +963.5769%] |

That substantial regression is the cost of validating previously trusted history
after each external commit. Larger histories or blobs can cost more. This is a
correctness fix with a measured shared-database tradeoff, not a universal speedup
or a new overall Penelope overhead percentage. It does not supersede earlier
operation-specific measurements made without this invalidation condition.

## Reproduction

See `docs/verification-data/2026-10-31-task-cache-revision/README.md`. The archive
contains source snapshots, binary/source hashes, all raw runs, statistics and
qualification logs. Its verifier checks integrity, baseline failures, all final
regression runs and independently recomputes the 21 paired comparisons.

Qualification passed: stable workspace/all targets/all features excluding the
separately configured fuzz package; Rust 1.85 SQLite all targets/all features
(24 unit, 51 process and 37 task tests plus custom bench executables); strict
workspace Clippy; formatting; layer boundaries; and graph update. The preceding
commit passed CI run 41172725798. This report does not claim completed CI for
the new commits.
