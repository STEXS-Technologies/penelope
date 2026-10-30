# Static task frame inventory

`python3 scripts/record-task-stack-usage.py target/task-stack-inventory` records
compiler-reported function frames as a separate source of memory evidence from
Massif samples. It does **not** yet establish a complete supervisor stack bound.

The tool extracts the fixed journal/runtime and five task exercises from the
portable allocation fixture into a temporary, nonpublishable `no_std` library.
It retains the 64 KiB positive control and removes the hosted allocator/main.
The library entry file contains only attributes, a module declaration and a
re-export. Dependencies retain the repository's locked versions; substitutions
are rejected. The owning adapters, test exercises and callback implementations
are part of this concrete profile, rather than arbitrary consumer ports.

Rust 1.99.0 compiles native x86-64 release code with one codegen unit, aborting
panics and red-zone use disabled. A rustc wrapper uses LLVM's stack-usage output
option for the fixture and runtime dependencies. It replaces itself with rustc
so Cargo's jobserver descriptors remain intact. Build tools and proc macros are
excluded from the runtime inventory. The wrapper forces one codegen unit for
instrumented crates to prevent concurrent writes to one usage file. It does not
change repository manifests, dependencies, installed toolchains or global settings.

The output preserves compiler classifications, function names, raw usage files,
compiler invocations, the generated source/lock, build output, fixture library
and hashes. Malformed records or a missing positive control fail the run. The
initial smoke records 89 frames and a 65,560-byte positive-control frame.

Individual frame sizes are not active call-chain sums. Inlining moves stack
ownership between functions. Return-address/ABI costs, indirect calls, recursive
paths, port callbacks, precompiled core/compiler-builtins, panic handling and
external memory primitives need separate accounting before claiming a complete
worst-case bound. Some recorded dependency functions may be unreachable from a
particular root. Neither summing every entry nor taking the largest entry proves
a bound. This explicit build profile also differs from the default hosted build.

This inventory supports further call-chain analysis; it leaves the worst-case
qualification item open. The [portable samples](performance-portable-memory-2026-10-30.md)
and [hosted samples](performance-hosted-borrowed-restore-2026-10-30.md) remain
observational evidence with their original compiler/profile scope.

The subsequent [freestanding RISC-V reference analysis](portable-task-stack-bound-2026-10-30.md)
resolves a complete call graph and sums its frames for one explicit fixed-port
profile. The native frame inventory above retains its original, narrower scope.
