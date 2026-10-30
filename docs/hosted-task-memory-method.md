# Hosted supervisor memory comparison

`scripts/measure-hosted-task-memory.py` builds the same reviewed fixture against
exact baseline and candidate revisions. It archives each revision, adds only the
fixture example, and builds separately with Rust 1.99.0 and each revision's locked
dependencies. It does not change the working checkout or dependency versions.

The fixture uses fixed in-memory ports. It asserts pure checkpoint recovery and
effect reconciliation through Unknown, reconstruction, explicit restart authority
and completion. It verifies output bytes, start/fence counts and exactly one
checkpoint ACK. A calibrated global allocator checks allocation, zeroed allocation,
reallocation and deallocation before asserting zero calls/bytes across the entire
lifecycle. CLI parsing and reporting are outside the allocation interval.

The runner measures native wall/user/system time and peak process RSS separately
from instrumented stack samples. Each paired stack run includes startup and
64 KiB positive controls. Invalid workload/allocation output, malformed stack
reports, missing calibration and nonfinite resource values stop the run. It
retains generated source, builds, toolchain/lock/binary hashes, raw stdout/stderr,
GNU time records and Massif profiles. Paired revision order alternates by round.

For example, after other stress jobs have finished:

```sh
mkdir -p target/qualification-temp
TMPDIR="$PWD/target/qualification-temp" python3 scripts/measure-hosted-task-memory.py \
  target/hosted-memory-4096 --before edcc5b423e4919f51e4aa2cb820d9de68a684145 \
  --after HEAD --cpu 0 --capacity 4096 --runs 3 --repetitions 10000
```

Repeat with a new output directory and `--capacity 65536` to observe capacity
scaling. Output directories must not already exist. Both scenarios use the same
4-byte input and 8-byte checkpoint/output; capacity changes reserved wrapper
workspace, not the logical dataset.

Only smoke validation has run so far. No qualification result is claimed here.
Whole-process sampled maxima include fixture ownership/moves and startup; they
are not a static worst-case bound or isolated supervisor cost. The fixed ports do
not characterize SQLite or process-runtime allocation. OS caches and frequency
remain uncontrolled, so these are warmed process runs without a cold-cache claim.
