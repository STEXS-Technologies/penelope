# SQLite process head/tail consistency

Code: `b746d3e`; baseline: `837265bfc82658a6d1fe02e4776372fe4673d805`.
Full source pins, patch, compressed logs, and verifier are in
`verification-data/2026-10-31-process-frontier/`.

Replay previously checked the head and requested page without checking that the
stored head agreed with the last indexed outcome. A cursor could skip damaged
history and receive an empty or truncated successful page. Exact commit retries
and new appends also accepted some inconsistent states.

The adapter now reads the head, byte usage, and scoped maximum outcome sequence
in one SQL statement within the existing transaction. An unknown scope is empty
only when both head and outcomes are absent. A populated scope requires a
positive next sequence, nonnegative byte usage and tail, and `tail + 1 == next`.
Replay, receipt reconstruction, and new commits share this check. The prepared
statement is cached; its results are read anew on each invocation. There is no
schema or public API change.

## Reproduction and regression

Six mutations obeying schema CHECK constraints remove the head, lower or advance
it, remove the last or all outcomes, or insert an extra tail outcome. Each is
tested against four replay cursors, an exact original retry, and a new append:
36 independent fixtures. The baseline has 20 false successes, four Conflict
responses where Invariant is required, and two appends that change the damaged
database. The remaining 12 cases already return Invariant.

The candidate rejects all 36 with Invariant and preserves logical snapshots of
all ten process/lease tables. Each fixture is then restored by the test, the
original operation succeeds, and reopen/retry/full replay checks pass. Restoration
is test machinery, not production repair behavior. Separate healthy controls
cover unknown and inbox-only scopes while another scope has populated history.

Twenty isolated processes, ten each on stable and Rust 1.85.0, pass all three
focused tests: 720 damaged-state cases plus repair/reopen controls and 120 healthy
empty-page checks. Final complete process suites pass 43 tests on both compilers.
The stable workspace/all-target/all-feature suite, MSRV SQLite/all-target suite,
strict workspace Clippy, formatting, layer boundaries, and graph update pass.
Final focused suites were rerun after test-helper lint fixes.

## Query work and limits

An engine-only fixture extracts the exact production SQL and uses the actual
adapter schema with bundled SQLite 3.53.2. It returns the expected frontier at
histories of 1, 128, and 4,096 outcomes, with 8,192 synthetic outcomes in another
scope. Every case uses 37 SQLite VM instructions, zero sorts, a head index seek,
and a covering outcome index search. These synthetic records are used only for
query planning/work measurement, not public replay validation.

This establishes indexed access rather than a history scan for these fixtures.
It is not a public-port latency benchmark or a new application overhead estimate.
The extra consistency check may affect latency; no speedup is claimed here.
It does not scan checksums outside the requested page, detect every internal gap,
or detect coherent changes to both metadata and records.

Verify archived evidence from the repository root:

```sh
python3 docs/verification-data/2026-10-31-process-frontier/verify.py
```
