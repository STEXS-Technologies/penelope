# Timeline verification

This records the initial reference-model campaigns. The
[qualification audit](qualification-0.4.0.md) records completed branch gates,
the repeated chaos campaign and later source-specific follow-ups. Tagging and
publication follow the [release contract](RELEASE.md#packaging-and-publication).
The model deliberately uses bounded integer computations and a cooperating mock
durable recipient. It cannot establish determinism or authority for arbitrary
consumer code, nor exactly-once delivery to an uncooperative endpoint.

## Reference model

`crates/penelope-timeline/tests/support/timeline_model.rs` is shared by five
property tests and five libFuzzer entry points. State correction and input
replacement use an independent closed-form oracle for `S' = 3*S + input` modulo
2^64. Metadata transitions are checked against independent reference fields.

| Campaign | Assertions |
| --- | --- |
| Projection | Revision changes, authority ordering, finality, slot limits, stale sessions, abort and restore without branch reuse |
| Plans | Exact post-step indexing, replacement identity/version, contiguity, all scope roles, quotas, finality and stale heads |
| Correction | Scratch replay at first/middle/latest boundaries, arbitrary yields, faults at replay offsets, intent origin, immutable old branches, expiry and materialization |
| Promotion | Two validated candidates, either winner, live append/authority/finality invalidation and explicit abort |
| Effects | Independently finalized prefixes, bounded ordered partial handoff, failure before commit, lost ACK after commit, mismatched receipt and identical retry |

Each trace is bounded to 64 operations; correction horizons are 1–16 steps.
Exhaustive tests enumerate 4,096 three-transition controller traces and 1,200
short correction/fault cases. Five property tests additionally run 1,000 cases
each with each of seeds 401, 402 and 403. All pass on the development host.
The complete timeline target tests also pass on stable and Rust 1.85.0.

`tests/determinism.rs` checks identical-baseline replay with hidden state,
complete captured state, an incomplete consumer restore and changed floating-point
reduction order. A digest mismatch rejects the candidate while preserving live
state. These tests demonstrate detection when validation is enabled, not an
ability to infer missing application dependencies.

The correction trace also drives owned-state `apply_input` and the scoped
correction API against the same closed-form oracle: cancellation, failed
validation/model execution, zero-budget yields, bounded progress and retry with
one reservation slot. The extended correction target passes 100,000 instrumented
runs with seed 405. Dedicated replay tests cover restore/model/validator unwinds,
ignored sink rejection, merged quotas, counter exhaustion and a retired-state
destructor panic after coherent scoped publication. A compile-fail test checks
the scoped handle's exclusive live-owner borrow. The quickstart runs in stable CI.

Correction validators receive the proposed publication head, consistently with
new-input validation. Regression tests compare every proposed field with the
subsequently published head on low-level and scoped paths. Restoring the old
behavior (passing the replaced head) makes the regression fail. Core tests cover
finality preservation, the maximum revision and rejection of an aborted token:
`BranchReservation::proposed_head` is validation metadata, not a commit receipt.

## Reproduction and failure inputs

Run from the repository root:

```sh
PROPTEST_CASES=1000 PROPTEST_RNG_SEED=403 \
  cargo test -p penelope-timeline --test model --all-features --locked
cargo test -p penelope-timeline --test determinism --all-features --locked
python3 scripts/seed-timeline-corpus.py
cargo +nightly fuzz run fuzz_timeline_correction -- -runs=10000 -seed=403
PENELOPE_FUZZ_RUNS=1000 PENELOPE_FUZZ_SEED=403 ./scripts/run_bounded_fuzz.sh
```

The last command retains every existing fuzz target and includes all five timeline
targets. `run_pure_chaos_drill.sh` runs both executor and timeline properties with
rotating seeds. The corpus seed script adds hash-named deterministic traces,
including full windows and fault boundaries; it never deletes mutated inputs or
overwrites conflicting files. Script tests verify those guarantees.

Property failures persist reproducible seeds under `proptest-regressions/`;
libFuzzer writes exact crashing inputs under `fuzz/artifacts/`. CI preserves
these on failure for stable, MSRV and fuzz jobs. Replay a fuzz artifact with
`cargo +nightly fuzz run <target> <artifact-path>`. Keep the same commit,
toolchain and corpus for campaign reproduction; mutation seeds alone do not
identify a corpus that has grown across previous runs.

The initial five instrumented state-machine targets each passed 10,000 runs with
seed 403 after deterministic corpus seeding, followed by 100,000 runs each with seed 404
against the accumulated corpus. These runs used stable rustc 1.99.0 for properties
and nightly rustc/cargo 1.99.0 for libFuzzer. Local logs are under the ignored
`target/timeline-foundation/` directory (`model-final-seed*.log`,
`fuzz-seeded-*.log`, `fuzz-100k-*.log`). These bounded campaigns supplement existing
golden, resource, panic, concurrency, portable and durable-adapter tests. They are
not a proof over arbitrary histories. The qualification audit links the committed
gate archives, completed repeated campaign and later optimization-specific logs;
the local paths above describe the initial campaign's working files.
