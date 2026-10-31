Run `python3 docs/verification-data/2026-10-31-task-replay-memo/verify.py` from
the repository root. It uses only Python's standard library and recomputes both
bootstrap campaigns in a temporary directory, leaving the archive unchanged.

For fresh measurements, use a separate clean checkout of candidate code commit
`f583fe9` with its Cargo.lock. Copy build.py, compare.py, analyze.py, confirm.py,
prepare-confirm.py, analyze-confirm.py and repeat.py into target/task-replay-memo/.
Run build.py from the checkout root. It temporarily substitutes the pinned
pre-guard, guarded-baseline and candidate task sources, builds all three benchmark
executables for each, then restores candidate source in a finally block.

Run compare.py, then analyze.py for the primary campaign. Run prepare-confirm.py,
confirm.py, then analyze-confirm.py for the three-version confirmation. Keep
builds, tests and graph jobs stopped during measurement. Scripts pin CPU 0; adapt
and record the selected CPU if unavailable on the measurement host. Executable
hashes can vary across build environments; this recorded environment reproduced
the measured hashes after both campaigns.

Run repeat.py separately for the two regression suites: ten isolated processes
per suite per compiler. baseline-control.py and negative-control.py also operate
from the repository root and temporarily replace only task source in a finally
block. The latter deliberately introduces a digest-only memo and requires the
changed-byte-count regression to fail. It is an expected failure, not production
behavior. A final candidate test run should follow those control experiments.

PENELOPE_BENCH_ALTERNATING_BLOBS=1 selects alternating checkpoint identities for
reconstruction and allocation probes; the default repeats one checkpoint.
PENELOPE_BENCH_EXTERNAL_COMMIT=1 selects the commit profile with an unrelated
external write before every timed operation; it omits the 1 MiB size. These flags
are explicitly set/cleared by measurement orchestration.

The pre-guard provenance manifest is copied from the preceding qualification.
It pins the historical executable and unchanged commit benchmark; guarded binary
identity also matches that preceding campaign. Rebuild verification records that
all primary artifacts and the historical executable were reproduced. Executables
remain in ignored target storage; the archive retains source snapshots, hashes,
raw observations, analysis, regression and qualification logs. Gzip mtime is zero.
Historical evidence does not automatically qualify future production changes.
