# Portable task memory observations — 2026-10-30

The release-profile portable-core fixture executable was measured on CPU 0 with
Rust 1.99.0 and Valgrind 3.27.1. Three runs of each path produced identical maximum
retained Massif stack samples. All five paths also passed their zero-allocation
assertions while instrumented.

| Whole-process case | Maximum sampled stack, bytes |
| --- | ---: |
| Startup/argument-parsing baseline | 7,728 |
| Positive control with a live 65,536-byte local array | 66,456 |
| Pure checkpoint/recovery/completion | 85,792 |
| Effect reconciliation/recovery/completion | 85,792 |
| Undersized workspace rejection | 44,928 |
| Checkpoint migration/recovery/completion | 71,888 |
| Independent reconciled completion | 44,976 |

These are **whole-process samples**, including startup, assertions, logging and
fixture ownership/moves. They are not incremental core stack costs; subtracting
baseline would not isolate the core because the maxima can occur at different
points. Massif retains a finite set of samples. These observations are not a
static upper bound, exhaustive worst-case measurement or embedded-target stack
certification. The positive control verifies observation of a known large frame.

The x86_64 type layouts in the same executable are:

| Value | Bytes |
| --- | ---: |
| `TaskControl<(), ()>` (layout with zero-sized ports) | 416 |
| `TaskProjection` | 408 |
| `TaskWorkspace` descriptor (three borrowed slices) | 48 |
| Caller byte buffers in these fixtures | 20 |
| Fixed journal fixture | 13,224 |
| Runtime fixture | 16 |
| `TaskControl<Journal, Runtime>` | 13,656 |
| `TaskRecord` | 400 |
| `TaskDefinition` | 104 |

The controller owns its ports. Port storage therefore contributes to controller
size; zero-sized-port layout is not the memory use of a functional journal.
The large fixed journal and its moves contribute to fixture stack use. A consumer
must budget its chosen journal/runtime storage, caller workspace and call stack
separately. Caller buffers may be placed outside the stack. The descriptor and
controller types have no const byte-capacity parameter, and the portable control
implementation owns no input/output/checkpoint arrays. None of these numbers
characterizes the hosted wrapper, SQLite worker or Linux process runtime.

Reproduce from the repository root (output must not already exist):

```sh
python3 scripts/measure-portable-task-memory.py /tmp/portable-memory --cpu 0 --runs 3
```

The runner builds with `--release --no-default-features --locked`, pins each
instrumented process, calibrates each run, checks allocation assertions and
rejects missing/invalid stack profiles. A separate test covers malformed profile
handling. No global cache eviction is performed. This is a memory measurement;
Valgrind execution times are not application latency evidence.

[Raw profiles and metadata](performance-data/2026-10-30-portable-memory/) retain
21 Massif reports, stdout/stderr, build output, source/runner/binary/lock hashes,
tool versions, build environment, type layouts and the tracked source diff from
the recorded revision. `measurements.json` records each maximum and profile hash.
The fixture change is recorded as a diff from `aae51bb`; this report does not
claim that the measurement used an unmodified release commit.

The general worst-case stack bound and hosted supervisor before/after memory
qualification remain open release evidence requirements.
