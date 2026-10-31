# Task journal read frontier verification — 2026-10-31

Baseline: `7dcc9c84b42e00cbaf4c885aef0d531cd161b60a`.
Code: `90bce24` (`Reject task reads with inconsistent durable frontiers`).

A task journal read trusted its head when choosing between a retained record and end-of-log. A missing tail could therefore remain undetected when reading beyond that head. The baseline regression run also directly reproduced a stale lowered head returning `Ok(Some(record))` instead of corruption; it stopped at that first unexpected success.

Reads now compare the head's next sequence with the maximum retained sequence in the same transaction snapshot. A cached SQL statement performs the scoped lookup using the `(scope, sequence)` primary key. Missing heads with retained records, missing tails, unexplained tails, and invalid head metadata return `TaskPortError::Corrupt`. The head digest is checked as a bounded 32-byte BLOB without making an owned copy. Retired-scope rejection and selected-record checksum/decoding remain in place. No schema migration is required. Append, runtime authorization, retirement, and head rebuild code are unchanged.

The regression matrix independently injects nine forms of damage at four cursors (0, 1, 2, 99): missing head, lowered head, advanced head, missing tail, missing all records, extra tail, empty digest, TEXT digest, and oversized digest. Metadata fault injection temporarily disables CHECK constraints to simulate persisted damage; normal enforcement is restored immediately. Each of 36 cases must reject the read without changing the heads, records, or blobs. Restoring the authoritative fixture must permit both the same connection and a reopened journal to read successfully. Separate healthy controls alternate unknown and populated scopes and observe an append through another connection, exercising cached statement rebinding and fresh results.

Final fixtures passed in 20 isolated processes, ten per compiler (stable 1.99.0 and MSRV 1.85.0), totaling 720 corruption cases plus repair/reopen and healthy controls. The workspace all-target/all-feature run excluding `penelope-fuzz`, SQLite MSRV all-target/all-feature run, strict workspace Clippy, formatting, layer boundaries, and `graphify update .` passed. Broad test runs covered the original 24-case matrix; the final targeted runs cover the additional metadata cases and healthy controls.

This check does not validate every interior record or referenced blob, compare the head digest with the tail record, detect coherently altered head/tail pairs, or prove that a never-observed scope was not completely erased. Full reconstruction remains necessary for broader validation. No comparative latency campaign was run: these results establish a reliability improvement, not a speedup or a new application overhead percentage.

Raw logs and SHA-256 metadata: [verification data](verification-data/2026-10-31-task-read-frontier/metadata.json). Benchmark output in broad test logs is smoke validation only.

Reproduce the final regression tests with:

```sh
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo test -p penelope-sqlite --test tasks task_frontier
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo +1.85.0 test -p penelope-sqlite --test tasks task_frontier
```
