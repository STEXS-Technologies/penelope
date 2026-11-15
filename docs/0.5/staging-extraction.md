# Shared intent admission extraction

The machine and timeline now call one allocation-free metadata admission helper
in `penelope-core::__private`. It is internal, version-coupled support for sibling
crates, hidden from application documentation. It creates no new package or
feature dependency.

The shared responsibility is count-first, duplicate-second, checked-byte-last
preflight. The helper takes metadata iterators and typed error values, never a
payload. Both callers retain payload ownership, their public error categories,
slot checks, provenance and poisoning. Machine staging now accounts for the whole
retained-plus-tentative batch directly, removing the separate merge arithmetic.
Timeline storage, correction/finality policy and legacy wire records are unchanged.

Poisoning deliberately stays local. A failed machine stage refuses later stages
and retains the first error; timeline retains its existing sink behavior. Both
poison before rejected payload destruction, including when a consumer catches a
destructor panic. Unifying those policies would change observable behavior.

The differential test preserves the original timeline and machine admission
algorithms as independent references. It covers capacities zero/one/four, every
retained/tentative split, count limits, duplicates, byte limits and integer
boundaries. Existing lifecycle, correction, effect-sequence and panic tests remain
in place. The dependency gate enforces the existing core/timeline direction;
regression fixtures now also cover reverse dependencies through the umbrella
facade and production infrastructure added directly to that facade.

## Equivalent-work performance check

Parent: `cce1217030006a7ee05a40c43cc0c0d025656a2b`. Baseline and candidate use
Rust 1.99, release defaults, identical existing `timeline_operations` workloads,
200 measured samples and 16 warmups per workload. Separately compiled executables
were copied and hashed, then run in alternating order for five rounds. No builds
or other qualification jobs were launched during those alternating runs. These
are local comparison measurements, not final release performance qualification.
The initial exploratory measurements overlapped builds and were inconclusive;
they are not used to establish a regression or improvement.

The table reports the median of five run-level p50 values, in nanoseconds.
The affected append profiles include one attempted new intent with sparse/dense
retained batches; full profiles exercise refusal as well as successful staging.

| Existing workload / pending count | Parent ns | Candidate ns | Change |
| --- | ---: | ---: | ---: |
| effects_append_cap8 / 0 pending | 261 | 260 | -0.4% |
| effects_append_cap8 / 1 pending | 270 | 270 | +0.0% |
| effects_append_cap8 / 8 pending | 250 | 241 | -3.6% |
| effects_append_cap64 / 0 pending | 601 | 592 | -1.5% |
| effects_append_cap64 / 1 pending | 611 | 602 | -1.5% |
| effects_append_cap64 / 8 pending | 682 | 681 | -0.1% |
| effects_append_cap64 / 64 pending | 441 | 431 | -2.3% |
| effects_append_cap512 / 0 pending | 5310 | 5360 | +0.9% |
| effects_append_cap512 / 1 pending | 5310 | 5371 | +1.1% |
| effects_append_cap512 / 8 pending | 5380 | 5431 | +0.9% |
| effects_append_cap512 / 64 pending | 5911 | 5952 | +0.7% |
| effects_append_cap512 / 512 pending | 3847 | 3857 | +0.3% |

All candidate median p50 values fit within or below their corresponding observed
baseline p50 ranges. This supports retaining the extraction; it does not establish
a universal regression tolerance or a statistically proven speedup. Raw p50/p95/p99
runs, binary hashes and source hashes are preserved in the checkpoint evidence.
Allocation and memory records match the parent exactly, including zero allocation
for the affected staging operations. The machine allocation probe also retains its
positive control and zero-allocation lifecycle requirement.

Reproduce the alternating runs with separate parent and candidate checkouts:

```sh
python3 docs/0.5/evidence/staging-extraction/compare.py \
  --before /path/to/cce1217-checkout --after /path/to/candidate-checkout \
  --out /tmp/penelope-staging-reproduction
```
