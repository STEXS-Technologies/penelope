# Paired application workloads

Run the suite from the repository root:

```sh
python3 scripts/benchmark-application-workloads.py
```

The runner builds the release executable once, then runs it without concurrent project builds, tests or allocation probes. It saves every paired sample, configuration, CPU affinity, storage filesystem, source/binary hashes, toolchain information and a validated summary under a new `target/application-workloads-TIMESTAMP/` directory. Use `--output PATH` to choose a destination; existing directories are refused. SQLite databases use that directory's filesystem, rather than silently defaulting to `/tmp`.

Default campaign: four process repeats, eight alternating direct/managed rounds, four workflows per worker per round, kernel work sizes 4096/65536/1048576, and one/four workers. This produces 1,152 paired samples across 36 profiles. On machines with fewer than four available CPUs use `--workers 1`. Smaller initial checks can use `--repeats 2 --rounds 2 --iterations 1 --work 4096 --workers 1`.

Each workload uses the same non-inlined deterministic CPU kernel, runtime inputs, call count and output checksum in both paths. Direct execution performs the business computation without orchestration or reliability services. Managed execution runs the public Penelope APIs, requires completion, validates outputs and fails on any error. This measures the cost of adding the declared services to this synthetic business workload. It does not compare two implementations offering equivalent durability guarantees.

| Case | Business calls per workflow | Managed services measured |
| --- | ---: | --- |
| `linear` | 8 | Eight typed steps, decisions, result correlation, volatile event history, terminal validation |
| `graph` | 4 | Branch on success/terminal failure, join, finish; both branch routes are checked |
| `retry` | 9 | Eight completed steps plus one failed attempt, bounded immediate retry with a new action identity |
| `replay` | 8 | Linear workflow plus projection reconstruction halfway through from volatile event history |
| `durable` | 8 | SQLite event/output payloads, inbox acceptance, atomic outcome/action commits, outbox claim/ack, terminal paged replay and business-result verification |
| `durable_replay` | 8 | Durable workflow plus midpoint close/reopen, projection reconstruction and business-value restoration from verified persisted payloads |

Durable writes use the normal SQLite adapter configuration (WAL/FULL). Action results are committed before their outbox leases are acknowledged. Event payloads bind the associated business-result digest. Recovery reads those referenced payloads rather than continuing from the old running value; a missing referenced result fails a regression test. The fixture tracks committed sequence numbers and reconstructs that frontier after reopening. It does not read the full history before every normal append.

Database creation/migration, definition-fixture construction and final fixture teardown are outside timing. The midpoint reopen and replay are inside timing. Completion checks, payload verification, durable terminal replay and checked results are included in managed time. This is an operationally checked integration workload, not a measurement of bare engine calls alone. Reopening is a clean restart/reconstruction exercise, not a process-kill or power-loss experiment. The volatile replay case reconstructs the decision state only.

Single-worker execution runs on the calling thread. Parallel execution includes spawning and joining the same number of business threads in both paths, with independent process streams and independent databases per worker. It measures aggregate completion time and throughput; it does not measure contention on one shared SQLite writer. CPU affinity selects the first available CPU for serial runs and the first N available CPUs for N workers. CPU frequency, host background load, OS cache state and filesystem scheduling remain uncontrolled. Detached database-worker cleanup can overlap subsequent work; alternating order and process repeats reduce, but cannot remove, host noise.

## Reading the output

`summary.json` reports each case/work-size/worker-count separately: direct and managed median cost per completed workflow, paired median added time, geometric paired overhead, throughput, and 95% bootstrap intervals resampled over complete process-repeat blocks. Four repeats provide limited uncertainty evidence; intervals are descriptive, unadjusted per-profile intervals, not guarantees. In parallel profiles, normalized time per workflow is throughput-derived cost, not individual request latency. No p95/p99 request-latency claim is made.

The formula is `100 * (managed_time / direct_time - 1)`, calculated from matched batches. A very short CPU task can show large percentages despite a small absolute cost. Durable cases also include the added reliability guarantees missing from the direct baseline, so their percentages must not be described as pure engine inefficiency.

`weighted_estimates` gives a synthetic mixed-workload estimate separately for each work size and worker count. By default, it assigns equal workflow counts to all six cases. It weights measured direct and managed costs before taking their ratio; it does not average path percentages. This is an estimate from separately measured cases, not a measured interleaved mix or a universal Penelope overhead figure. Different business durations, storage, concurrency and traffic mixes change it.

Provide your own mix with a JSON file containing a nonnegative weight for every case:

```json
{"linear": 70, "graph": 15, "retry": 5, "replay": 5, "durable": 4, "durable_replay": 1}
```

```sh
python3 scripts/benchmark-application-workloads.py --mix /path/to/mix.json
python3 scripts/application_workloads_summary.py target/application-workloads-TIMESTAMP
```

The summarizer refuses missing or duplicate samples, unverified results, inconsistent business checksums, unexpected profile dimensions and invalid weights. Its known-ratio and negative-control tests run with:

```sh
python3 -m unittest discover -s scripts/tests -p test_application_workloads_summary.py
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo test -p penelope-executor --test application_workloads
```

The Cargo benchmark's default invocation is a small correctness smoke run, not the reference campaign. For custom runs of the release executable, supported flags are `--case`, `--work`, `--rounds`, `--iterations` and `--workers`.

## Coverage limits

This suite covers representative normal, parallel, immediate-retry, replay and SQLite workflows. It does not cover every Penelope API or promise an application-wide slowdown. Protected Linux worker startup/IPC/checkpoints/fencing, timer backoff, manual review, compensation, external network/database effects, authorization services, shared-store writer contention, true crash recovery, allocation/RSS peaks and OS-page-cache-cold reads require separate scenarios. Existing dispatch, task-journal, memory and fault-injection benchmarks remain complementary evidence, not interchangeable overhead percentages.

## Atomic admission option

```sh
python3 scripts/benchmark-application-workloads.py --admission atomic
```

The default remains `--admission preaccepted`, preserving the original separate inbox acceptance step. Atomic admission omits that call and supplies the input directly to `AtomicProcessCommit`. The existing adapter accepts and consumes the input together with its outcomes/actions inside the same WAL/FULL transaction. The two modes produce identical successful durable rows and results. Atomic admission removes one separate transaction per input: nine transactions for this eight-step workflow.

Choose preacceptance when a durable pending input and a separate ingress receipt must survive a later decision-commit failure. In atomic mode there is no independently committed pending input if that commit fails; the producer must retain/redeliver the request until the atomic commit is acknowledged. Fault tests exercise this difference and successful retry after reopening. This is an application integration choice using an existing port contract, not weaker SQLite durability or a change to the production adapter.

Use `python3 scripts/benchmark-application-admission.py` for a balanced two-campaign comparison of both modes in the two durable cases. It retains raw rows and computes block-bootstrap intervals; `--summarize-only DIRECTORY` validates/recomputes an existing comparison. The main workload runner records the selected admission mode, and its summarizer rejects mixed-mode samples. Archived reference figures remain pinned to their original source version and preaccepted mode.
