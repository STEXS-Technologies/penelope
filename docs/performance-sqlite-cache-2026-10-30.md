# SQLite verified file-cache replay measurements

Five alternating-order pairs for each of three databases completed after the
previous fuzz process stopped and before the replacement campaign started.
All 30 replays verified the complete expected projection and referenced blobs.
Every cold observation had zero resident database pages; every warm observation
had all pages resident immediately before launching the measured process.

| Records | File pages | Cold median ms (range) | Warm median ms (range) |
| --- | ---: | ---: | ---: |
| 16 | 39 | 0.941 (0.906–1.069) | 0.771 (0.736–0.794) |
| 128 | 72 | 3.081 (3.027–3.159) | 2.799 (2.790–2.851) |
| 512 | 183 | 10.600 (10.579–10.681) | 9.949 (9.791–10.067) |

The internal timer covers adapter open, replay and close, excluding process
startup and cache preparation. These are five observations per mode, not a
population p95/p99 estimate or a claim about device-cold latency. The warm mode
also opens a fresh connection; it is not an already-open SQLite connection.

The reference was `0162c19`, Rust 1.99.0, release, locked dependencies, CPU 0,
SQLite 3.53.2, WAL and synchronous FULL on local NVMe-backed btrfs. No fsync
bypass or global cache eviction was used. The runner verified database hashes
unchanged after each replay. The [method](performance-sqlite-cache-method.md)
describes mincore verification, POSIX_FADV_DONTNEED, scope and limits. Database
file pages are the only cache domain qualified here; executable, metadata,
sidecar, controller and whole-machine caches remain outside the label.

Raw reports, per-pair cache observations, process timings, dataset hashes,
build logs and tool/filesystem provenance are retained under
[verification data](verification-data/2026-10-30-sqlite-cache/manifest.json).
The manifest records hashes for original and byte-exact compressed data.
