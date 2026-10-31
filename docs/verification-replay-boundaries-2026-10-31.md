# Maximum replay pages and cross-worker refresh — 2026-10-31

Added a public-adapter regression test for maximum replay page boundaries. Production code is unchanged from `8b6ff6d`.

Two independent SQLite store workers open the same database. The reader first warms an empty maximum-page request. The writer creates 512 outcomes through four public 128-outcome commits, then appends outcome 513 through another public commit. Before and after the append, the existing independent history-filter model verifies complete outcomes, scope, cursor and continuation for seven cursors (0, 1, 127, 128, 511, 512, 529) and five limits (1, 127, 128, 511, 512). This exercises full pages, partial tails, end and past-end reads, and cross-worker refresh after warming statements. Appends are sequenced between reads; this test does not claim an overlapping read/write race.

After healthy reads, an external connection corrupts the checksum of sequence 511: the last row of a maximum page, after 511 valid preceding rows. Both the warmed reader and a newly opened reader must return `PortError::Invariant`, with all ten process tables exactly unchanged. Both must still read the healthy suffix starting at sequence 512. Restoring the checksum restores the full table snapshot; warm and newly opened readers then return correct pages again.

## Evidence

Twenty isolated test executions passed: ten on stable Rust and ten on Rust 1.85.0. They cover 1,400 cursor/limit comparisons and 40 typed corruption rejections, plus suffix and repaired-page checks. The full process suite passed on both toolchains: 61 tests each. Strict workspace Clippy, formatting and layer checks passed; the local graph was updated.

A deliberate control changed only replay decoding to recompute the expected checksum from the stored payload instead of trusting the stored checksum. The test failed on the first damaged maximum-page assertion: it received a successful page instead of `Invariant`. The production source was restored before successful qualification. This establishes that the new test detects checksum bypass at the final row; it does not claim coverage of every possible corruption.

Raw logs, the control description, source hashes and a standalone verifier are in [the evidence directory](verification-data/2026-10-31-replay-boundaries/). Run `python3 docs/verification-data/2026-10-31-replay-boundaries/verify.py` to check the retained results. This is reliability coverage, with no new speed claim.
