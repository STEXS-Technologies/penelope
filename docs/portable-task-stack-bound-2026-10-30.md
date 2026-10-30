# Fixed-port RISC-V task stack bound

The freestanding reference binary has a conservative **115,056-byte stack
bound** from `_start`, including its caller frames inside the binary and every
reachable callee. This closes the call-chain accounting gap for this concrete
reference profile. It does not establish a universal bound for user adapters,
other targets or ordinary hosted builds.

Run the reproducible build and analysis with:

```bash
python3 scripts/bound-portable-task-stack.py target/riscv-stack-qualified
```

The runner requires the installed Rust 1.99.0 toolchain, its
`riscv32imafc-unknown-none-elf` target, locked dependencies available offline,
and `llvm-objdump`. It preserves dependency versions and rejects build warnings.
The binary is release, one codegen unit, panic abort, no red zone and no jump
tables. It compiles the same fixed Journal/Runtime ports and five lifecycle
exercises as the allocation fixture. A selector also reaches a separate 64 KiB
positive control, whose compiler-reported frame is **65,552 bytes**. The
freestanding entry and volatile bytewise C memory primitives avoid host libc
and unresolved memory callbacks. The panic handler only spins; it does not
invoke formatting callbacks.

The analysis resolves RISC-V direct calls and AUIPC/JALR pairs using instruction
addresses, rather than trusting symbol annotations. All **42 reachable
functions** are inventoried. Unresolved indirect transfers, external targets,
interior cross-function entry, traps and recursion reject the result. Jump
tables are disabled in this explicit reference profile so switch lowering does
not leave opaque register jumps.

Exact-build LLVM `.su` records supply static frames for compiled dependencies
and the fixture. The remaining precompiled core functions are analyzed by
tracking immediate stack-pointer changes over their control-flow graphs.
Unaligned or unsupported changes, differing stack depth at a merge, and
stack-growing loops reject the result. Calls preserve the caller's stack
pointer under the RISC-V ABI. The analyzer sums the maximum static frame along
every reachable acyclic call chain, conservatively keeping caller frames live
even where a tail transfer might reclaim them. Duplicate local mangled symbols
remain distinct by address; each receives the largest corresponding exact-build
compiler frame, so merging cannot undercount a copy.

The largest conservative chain is:

| Function | Frame bytes |
| --- | ---: |
| `_start` | 16 |
| `qualify_task_path` | 41,968 |
| `lifecycle` | 67,888 |
| `restore_projection` | 3,648 |
| `TaskProjection::propose` | 848 |
| `TaskProjection::apply` | 672 |
| `TaskDefinition::eq` | 16 |
| `ExecutionBudget::eq` | 0 |
| **Total** | **115,056** |

This is an upper bound for the linked reference program's stack under the
compiler's static-frame reports and RISC-V ABI, starting with a valid
16-byte-aligned stack pointer. Boot code, interrupts, caller frames outside
`_start`, other execution contexts, hardware memory maps and execution success
on a board are outside its scope. Its fixed-port tests bound the selected
operations; arbitrary ports, buffer capacities and compiler settings require
rebuilding and reanalysis. Allocation and sampled hosted memory measurements
remain separate evidence.

The analyzer's tests cover nested and alternative paths, duplicate local
symbols, signed AUIPC calls, CFG merges, stack-growing loops, unsupported SP
writes, alignment, indirect calls, traps, missing/interior targets, recursion
and nonstatic/conflicting frame inventories. The complete Python suite passed
49 tests.

Raw generated source, entry, manifest/lock, ELF, disassembly, compiler
invocations/frames, bound and provenance are retained byte-for-byte in gzip
form under [verification data](verification-data/2026-10-30-riscv-stack-bound/manifest.json).
The manifest includes original and compressed SHA-256 hashes. This is stronger
than the earlier [individual-frame inventory](task-stack-usage-method.md),
which intentionally did not claim a call-chain bound.
