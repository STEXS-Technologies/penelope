# Penelope 0.3.0 pre-merge audit — 2026-10-31

This review checks the reliable-execution branch before merging it into `main`.
The reviewed production source is the tree at historical commit `7278eca`.
The audit changes release documentation and commit subjects; it adds no runtime
optimization or behavior change. No additional merge-blocking runtime defect
was identified in the reviewed paths. That is a scoped review result, not a
proof that the implementation contains no defects.

## Findings corrected

1. **Incomplete upgrade instructions.** The SQLite adapter guide described
   schema v5 but omitted the current v6 pending-inbox index. It now states the
   current version, transactional upgrade/page-limit behavior, and prohibition
   on opening upgraded databases with older adapters. It also explains pending
   input discovery, its 64-input bound, non-causal ordering, concurrent readers,
   atomic consumption, and exact retry behavior under writer contention.
2. **Incomplete generated 0.3.0 release notes.** Completed changes on this branch
   remained under `Unreleased`, so the release-note generator would exclude
   them from v0.3.0. They now appear in the existing dated 0.3.0 section, alongside
   schema v6, pending discovery, and the qualified retry improvements. The
   generator was exercised against that section; this does not tag or publish
   a release.
3. **Inconsistent branch subjects.** Branch commits are normalized to
   `type(scope): summary`, retaining feature, fix, test, documentation, and
   performance distinctions. The rewrite preserves each commit's file tree,
   authorship, timestamps, message body, and parent order. Historical evidence
   is preserved as described below.

The preceding CI regression fix at historical `7278eca` remains part of the
review: the seeded duplicate inbox retry must finish before an external writer
releases its lock. Fresh admissions and commits still require writer admission.
The corrected harness retains serial-history and reopened-state comparisons;
100 stable and 100 MSRV repetitions cover 400 locked duplicate completions.

## Code and contract review

The review followed the knowledge graph into the task controller, saga/task
composition and service drivers, SQLite admission/commit/replay/authority and
lifecycle paths, database worker, and Linux image/registry/start/poll/fence
boundaries. It checked these interactions against the published contracts:

- Authorizing history commits before worker dispatch; fencing before successor
  admission; preserving ambiguity and reconstruction after commit/runtime errors.
- Binding leases, results, checkpoints, sources, and receipts to exact identities;
  requiring independent effect evidence rather than trusting transport or exit.
- Keeping missing-row insertion and quota charging transactional, including a
  recheck after read-only duplicate inspection; validating current stored types,
  bounds, checksums, and semantic input equality without caching answers.
- Holding consistent snapshots during authoritative replay and blob retirement
  reads; retaining exact commit constituent and consumption verification.
- Publishing database results before notifying executors, running application
  callbacks outside completion locks, and retaining cancellation ambiguity.
- Pinning worker image/registry identity, restricting the single-process worker
  profile, bounding frames and writes, and confirming termination before a fence.
- Keeping optional adapters out of the default facade and keeping the task core
  usable without allocation on the declared portable target.

The review does not replace the existing corruption, concurrency, subprocess
kill, fault-injection, model, and fuzz suites. It does not inspect every archived
experimental source as if that source were shipped production code.

## Verification

All seven jobs in the [pre-rewrite PR CI run](https://github.com/STEXS-Technologies/penelope/actions/runs/41221203285)
pass, including stable, Rust 1.85.0, bounded fuzzing, portable core, optional
StateChronicle conformance, supply chain, and secret scanning. That run belongs
to its recorded source revision, not automatically to a future branch head.
The earlier full-workspace local test and strict Clippy checks pass on the same
production source.

Additional local audit checks pass:

- Python script unit tests, layer checks, and pinned-action checks.
- Workspace doctests and rustdoc with warnings denied.
- RISC-V `penelope-core` checks without default features, with and without Serde.
- All nine library archives built and verified from unpacked packages through
  the publication script's **dry-run** mode. No registry upload was performed.
- All 271 scanned local file links across 164 tracked Markdown files resolve;
  the six scanned Markdown heading links also resolve. External URLs and links
  generated dynamically by tools were not exhaustively checked.
- v0.3.0 changelog release-note extraction includes the moved branch changes.

Final-head CI must still pass after the documentation and history updates.
Existing benchmarks remain workload-specific: lightweight execution percentages
exclude durable storage, durable timer workflows include material I/O cost, and
unfavorable results and inconclusive controls remain part of the evidence.

## Historical evidence after message normalization

Rewriting commit messages changes commit IDs even when file trees are identical.
The pre-rewrite history is retained on
`archive/reliable-execution-premerge-2026-10-31`. The
[old-to-new commit map](verification-data/2026-10-31-premerge-history-map.json)
records both identities and each shared tree. Run the archived check with
`python3 docs/verification-data/2026-10-31-premerge-history-verify.py`.
Pinned hashes, raw datasets, old CI
links, and benchmark snapshots retain their original revision meaning; they are
not relabeled as fresh results.

A normal clone that fetches all branches obtains the historical objects. For a
single-branch clone, fetch them explicitly before using an evidence verifier
that invokes `git show` on a historical revision:

```bash
git fetch origin refs/heads/archive/reliable-execution-premerge-2026-10-31:refs/remotes/origin/archive/reliable-execution-premerge-2026-10-31
```

Historical verifiers that also compare the working tree must run in a disposable
checkout of their pinned candidate, rather than being interpreted as checks of
all subsequent edits. Their source/environment pins remain intentional limits.

The supported release boundary remains single-host local-filesystem SQLite and
the documented trusted Linux kernel/runtime profile. Application correctness,
authenticated storage, unknown external effects, network/distributed fencing,
and power-loss certification remain outside the demonstrated guarantees.
