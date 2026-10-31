Run `python3 docs/verification-data/2026-10-31-task-cache-revision/verify.py`
from the repository root. Python's standard library is sufficient. The verifier
recomputes all 21 confidence intervals, taking several seconds.

For a fresh comparison, use a separate clean checkout of candidate code commit
`f0c68ce` with its Cargo.lock. Copy build.py, compare.py, analyze.py and repeat.py
into `target/task-cache-revision/` and run them from the checkout root in that
order. Build temporarily substitutes the pinned baseline, uncached-counter and
cached-counter task sources and restores the candidate in a finally block. All
three executables use the identical candidate benchmark. Run no builds, tests
or graph updates concurrently with compare.py. It pins CPU 0; adapt and record
that CPU if unavailable on the measurement host. repeat.py is separate from
measurement and runs the final public-port regressions ten times per compiler.

The benchmark's default profile has no intervening external writes. Set
PENELOPE_BENCH_EXTERNAL_COMMIT=1 to force one unrelated-table commit on another
connection before every timed append. External writes are outside timing;
resulting journal replay is inside. That profile omits the 1 MiB size.

Raw timing and regression logs are gzip-compressed with mtime zero. Binary hashes
identify executables retained in ignored target storage. Production snapshots
and source hashes pin the measured implementation. The manifest records the
final benchmark and regression source hashes. Identical binary reproduction
requires the same build environment. Historical measurements do not qualify
later production source changes automatically.
