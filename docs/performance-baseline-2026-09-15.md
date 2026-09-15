# Library performance baseline: 2026-09-15

Revision: `a8a42a6`  
Command: `cargo bench -p penelope-ports --bench contract_throughput -- --noplot`

The optimized Cargo bench profile measured:

| Operation | Workload | Result |
| --- | ---: | ---: |
| Canonical DTO encoding | 100,000 operations | 743 ns/op |
| Bounded outcome-log append | 4,096 operations | 861 ns/op |

These are pure in-process contract costs on the current host. They exclude
storage, network, scheduling, lock contention, serialization outside the tested
path, and adapter retries. They must not be interpreted as durable or
end-to-end throughput guarantees. Re-run on the target deployment hardware
before setting an SLO.
