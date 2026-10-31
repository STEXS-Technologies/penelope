Run `python3 docs/verification-data/2026-10-31-task-blob-commits/verify.py`
from the repository root to check the saved evidence. Only Python's standard
library is required. `.gz` log/source files use deterministic mtime zero.

For a fresh timing comparison, use a separate clean checkout of code commit
`51b67d9` with the same Cargo.lock. Copy `build.py`, `compare.py`, `analyze.py`
and `repeat.py` from this directory into `target/task-blob-commit-comparison/`.
From the checkout root run that directory's build.py, then compare.py, then
analyze.py. The build script temporarily substitutes the baseline task source,
restoring candidate source in a finally block. It compiles the identical new
benchmark against both production versions. compare.py requires CPU 0 affinity;
adapt and record the selected CPU if this machine's allowed CPUs differ. Keep
other builds and tests stopped during comparison. repeat.py runs the final two
regression tests ten times per compiler and is separate from timing.

Binary hashes identify the measured release executables, which are retained in
ignored target storage rather than committed. Source snapshots and hashes pin
the production change. The manifest also hashes the benchmark source; recreating
identical machine-code hashes requires an identical build environment.
