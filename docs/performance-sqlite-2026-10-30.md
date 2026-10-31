# SQLite store baseline: 2026-10-30

Initial measurements for the optional SQLite adapter on production revision
`ddadbed`. This is a current baseline; it does not establish a before/after
comparison with v0.2.0, which did not include this adapter.

Benchmark source SHA-256: `dd2a1b727aca20fa8a825f53edcd1a7740090d1412c47f8113bb217e61fcf456`.
[Host metadata](performance-data/2026-10-30-sqlite/metadata.json) records the
lockfile hash and tracked diff. The same directory retains all three raw runs
and their resource reports.

Reproduce with:

```bash
python3 scripts/benchmark-durable-store.py /tmp/penelope-store-results \
  --data-parent "$PWD/target" --cpu 0 --runs 3
```

The runner builds once with Rust 1.99.0, then executes the optimized benchmark
on CPU 0 (AMD Ryzen 9 7950X), including its database-worker thread. The CPU is
pinned but not reserved exclusively. Data resides on `/dev/nvme0n1p3`, btrfs
with zstd compression; `/tmp` is tmpfs and holds reports only. SQLite 3.53.2
uses WAL and `synchronous=FULL`, without an fsync bypass. SQLite auto-checkpointing
and OS cache management use their defaults. No global page-cache eviction occurs.

Each task history starts with a 64-byte input and a start record, followed by
checkpoints referring to the same immutable 64-byte blob. History sizes are
16, 128 and 512 records, giving 14, 126 and 510 measured checkpoint appends.
Replay validates each record, reads/verifies each referenced blob and rebuilds
the exact projection. Ten warm replay samples and ten reopened-connection
samples run per history size. Opening a connection does not make the OS cache
cold. Database creation/migration timing is retained separately in the raw JSON.

The process workload appends 128 single-outcome/single-action commits in one
scope, each with a unique input and canonical-source identity. Each action is
claimed and ACKed, followed by an exact commit retry. Claim-to-ACK includes two
durable store operations and async worker scheduling; no external effect or
protected worker runs in this workload. Every receipt and task replay is checked.

Values below are milliseconds: the median of the three runs' individual
nearest-rank percentiles, not percentiles pooled across runs. With ten replay
samples, p95 and p99 both select the maximum. These small samples describe this
host and workload; they are not throughput or production latency guarantees.

| Operation | p50 ms | p95 ms | p99 ms |
| --- | ---: | ---: | ---: |
| Checkpoint append, 16 records | 0.671 | 1.358 | 1.358 |
| Warm record/blob replay, 16 records | 0.257 | 0.367 | 0.367 |
| Open + record/blob replay, 16 records | 0.388 | 0.524 | 0.524 |
| Checkpoint append, 128 records | 0.661 | 0.813 | 1.208 |
| Warm record/blob replay, 128 records | 2.084 | 2.265 | 2.265 |
| Open + record/blob replay, 128 records | 2.322 | 2.501 | 2.501 |
| Checkpoint append, 512 records | 0.635 | 0.846 | 1.465 |
| Warm record/blob replay, 512 records | 8.446 | 8.848 | 8.848 |
| Open + record/blob replay, 512 records | 9.072 | 9.592 | 9.592 |
| Process commit, 128 samples | 0.805 | 1.123 | 1.767 |
| Exact commit retry after ACK, 128 samples | 0.055 | 0.074 | 0.079 |
| Outbox claim through ACK, 128 samples | 1.283 | 1.849 | 2.626 |

Whole-process wall time was 0.96–1.07 seconds and peak RSS was 6,444–6,688 KiB
(`/usr/bin/time`, including the database worker). Compilation is excluded.
The benchmark's internal wall timer excludes temporary-directory cleanup;
the external wall timer includes process startup and cleanup.

Remaining performance qualification includes identical before/after core/saga
measurements, allocation counts, cold-cache deployment measurements, actual
protected-worker dispatch-to-ACK timing, and worst-case supervisor memory/stack.
This report does not close those release requirements or certify power-loss
behavior of the storage device.
