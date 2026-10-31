# Commit retry constituent encodings — 2026-10-31

SQLite commit retries reconstruct their receipt from current durable evidence. The saved commit digest alone is insufficient: verification checks outcomes, actions, input consumption, source binding, and the history frontier. An audit identified repeated JSON deserialization as a possible optimization target. This change adds compatibility and corruption tests before attempting that optimization; production code is unchanged and no performance improvement is claimed.

The existing corruption suite already rejects missing/substituted constituents, wrong SQLite storage classes and oversized BLOBs, damaged checksums, and broken consumption/source bindings. Two new integration tests add **60 cases** that distinguish valid noncanonical encodings from invalid evidence.

## New cases

The encoding matrix covers each of three constituent kinds—outcome, action, and input—with canonical, pretty, Unicode-escaped, and reordered JSON. These valid encodings must return an exact duplicate receipt. Pretty JSON with a substituted payload digest, escaped JSON with a bad checksum, correctly checksummed malformed JSON, and canonical JSON with a bad checksum must return `Invariant`.

The matrix runs each case through a warmed store and a reopened store: 3 kinds × 8 encodings/faults × 2 store states = **48 cases**. Outcome/action changes target the second record of a two-record batch, ensuring earlier statement reuse succeeds before the changed evidence is read. Each batch also carries a canonical source binding and a consumed input. Before changing bytes, an exact retry warms every verification statement. This checks that statement reuse cannot retain a stale validation result.

A second matrix pads valid JSON with whitespace to exactly 4,096 or 4,097 bytes under a 4,096-byte stored-record limit. The first must succeed and the second must return `Invariant`, despite matching checksums and semantic equality: 3 kinds × 2 sizes × 2 store states = **12 cases**.

Every observation compares all ten process/lease-counter table snapshots before and after retry. Successful receipts must validate against the original batch, identify a duplicate input, and report committed-through sequence 1. Rejected retries must leave the same snapshots unchanged.

## Fault sensitivity and qualification

Twelve temporary runtime mutations test fault sensitivity independently for each constituent kind. The four mutations skip checksum verification, skip semantic comparison, accept only byte-identical JSON, or bypass the stored-record bound. Each must fail its targeted test and identify the intended constituent. The runner restores exact production source bytes in `finally`; both new tests pass again afterward.

Stable workspace tests, SQLite/executor/process-runtime tests on Rust 1.85.0, strict workspace Clippy across all targets/features, MSRV all-target checks, formatting, and layer checks pass. Portable core checks with no default features, with and without Serde, also pass. Existing durable partial timer-batch recovery coverage remains included. The graph was updated after source restoration.

[Archived evidence](verification-data/2026-10-31-commit-retry-encodings/) includes source hashes, the mutation/qualification scripts, the complete twelve fault logs, restored test output, and qualification logs.

```sh
python3 docs/verification-data/2026-10-31-commit-retry-encodings/verify.py
```

The verifier checks source provenance, all fault records, restored source identity, and qualification markers. To repeat fault injection, use a disposable checkout at this commit and run the archived `negative.py` from the repository root. It temporarily modifies runtime source; do not run other project builds or tests concurrently. These cases establish required behavior for future retry optimizations; they do not prove every possible encoding or fault combination.
