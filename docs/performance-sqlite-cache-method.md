# SQLite file-cache replay method

`scripts/benchmark-sqlite-cache.py` adds a narrowly verified cold/warm comparison
for the optional SQLite adapter. Earlier [durable-store measurements](performance-sqlite-2026-10-30.md)
only reopened connections; they did not establish a cold OS file cache.

The runner builds the release `durable_store` benchmark once with Rust 1.99.0
and the locked dependencies. It prepares private 16-, 128- and 512-record task
databases on the selected filesystem using the existing WAL/FULL workload.
All SQLite connections close before cache control. Nonempty WAL/SHM sidecars
are rejected. Input/checkpoint bytes and expected projections are identical
between modes; SHA-256 checks reject changed database bytes after replay.

For each dataset, paired order alternates across rounds:

- **Cold file pages:** sync the owned file, request `POSIX_FADV_DONTNEED`, then
  map it privately and call Linux `mincore` without reading the mapped bytes.
  Every file page must be reported nonresident immediately before child launch.
- **Warm file pages:** read the entire owned file, then require every page to
  be reported resident by `mincore` immediately before launch.

A failed request, unavailable facility or partial residency rejects the label
and stops the runner. There is no global cache eviction or privileged operation.
No database read/hash occurs between residency verification and the measured
child launch. The next operation opens a fresh adapter connection and replays
all records/referenced blobs, comparing the full projection with the expected
one. Its internal nanosecond timer covers adapter open, replay and connection
close; it excludes cache preparation, expected-projection construction and
process startup. GNU time separately retains whole-process wall/CPU/RSS.

The word **cold** here describes only the benchmark main database's verified
file-page residency at the observation instant. It does not describe the NVMe
controller, device cache, executable, dynamic libraries, directory metadata,
whole machine, or SQLite sidecars newly created after launch. External host
activity and residency changes after the observation are outside its guarantee.
Do not compare these numbers directly with a warmed existing SQLite connection.

Run on an otherwise idle host after correctness/fuzz campaigns finish:

```bash
python3 scripts/benchmark-sqlite-cache.py target/sqlite-cache-qualified \
  --data-parent target/qualification-temp --cpu 0 --runs 5
```

Choose an existing private data-parent on the intended local filesystem and a
CPU in the available affinity mask. The temporary dataset directory is removed
on exit. Output retains raw replay reports, GNU time records, preparation output,
per-run residency vectors summarized as resident/total page counts, dataset and
binary/source/lock hashes, toolchain, filesystem and build environment. The
source diff is retained if measured before commit. Run labels alone are not
evidence: inspect every recorded residency check and dataset hash.

The development smoke run may overlap fault testing and is not qualification
latency evidence. The [serialized five-pair report](performance-sqlite-cache-2026-10-30.md)
records qualified file-page observations for the current adapter.
