# End-to-end synthetic application workloads, 2026-10-31

This campaign compares identical business computation executed directly and through six Penelope integration paths. It establishes scoped overhead figures and makes the cost of additional durability visible. It does not establish a universal Penelope slowdown or cover every supported API.

With approximately 60 microseconds of CPU work per business call (`work=65536`), the one-worker eight-step linear workflow has 0.588% measured overhead; graph 1.011%, immediate retry 0.430% and volatile projection replay 0.675%. The fully durable eight-step workflow costs approximately 48 milliseconds on this NVMe btrfs filesystem, versus approximately 0.48 milliseconds for business work alone. Its difference includes WAL/FULL writes, inbox/outbox operations, payload checks and terminal replay. It must not be presented as pure decision-engine overhead.

For the longer CPU workload, the linear, graph and retry overhead intervals include zero; volatile replay measures +0.319% with a [+0.216%, +0.401%] interval. These results do not prove a universal sub-1% bound. Shorter business tasks have higher percentages. The previous isolated one-step overhead figures and SQLite path optimizations answer different questions; they are not averaged into these results.

## Reproducible suite

See [workload documentation](application-workloads.md) for the scenario contracts, commands, timing boundaries, configurable weights and coverage limits. The suite uses current production source at baseline parent `e41d595`; exact benchmark, runner, lockfile and binary hashes are recorded in metadata. The benchmark was measured before its commit; source hashes identify the measured files and permit checking them against the committed versions.

The fixed corrected campaign has four process repeats, eight alternating direct/managed rounds, four workflows per worker, three CPU-work sizes and one/four workers. All 1,152 paired samples are retained, covering 36 profiles and 11,520 measured completed workflows on each side of the comparison (plus untimed warmups). Each profile has 32 paired samples across four process blocks. Twenty-four executable invocations were used. No project builds, tests, allocation probes or summarization overlapped timing. Definitions and initial database opens/migrations are outside timing; recovery reopen/replay is inside. Result and completion checks are inside the managed workload.

On Linux/Ryzen 7950X, CPU affinity used the first available CPU for serial and the first four for parallel. CPU frequency, unrelated background load, OS caches and filesystem scheduling were uncontrolled. Parallel streams use separate databases and include spawning/joining business threads in both paths. Reported parallel cost per workflow is derived from batch throughput, not individual request latency. Detached database-worker cleanup can overlap later work; results remain subject to host/teardown noise.

The direct baseline offers no durability. Percentages below include the declared extra services. The healthy/retry/replay paths do not invoke protected workers, network services or durable stores. Durable replay is a clean connection reopen with reconstruction of both engine projection and verified business state; it is not a process-kill or power-loss measurement.

## Results

Times are marginal medians in microseconds per completed workflow. Percentages use geometric ratios of paired managed/direct batch times, so the displayed medians need not reproduce them. Intervals resample complete process-repeat blocks 20,000 times with deterministic seeds, using unadjusted per-profile 95% intervals. Four blocks provide limited uncertainty evidence. All profiles, including slow and near-zero results, are retained.

| Workers | Kernel work per call | Path | Direct microseconds | Managed microseconds | Paired overhead | Process-bootstrap 95% interval |
| ---: | ---: | --- | ---: | ---: | ---: | --- |
| 1 | 4096 | linear | 29.845 | 32.215 | +7.575% | [+5.990%, +8.573%] |
| 1 | 4096 | graph | 14.920 | 17.231 | +16.269% | [+14.987%, +17.214%] |
| 1 | 4096 | retry | 33.584 | 36.084 | +6.987% | [+6.592%, +7.324%] |
| 1 | 4096 | replay | 29.843 | 33.347 | +12.093% | [+11.280%, +12.765%] |
| 1 | 4096 | durable | 29.992 | 48366.587 | +161746.177% | [+149433.814%, +175072.318%] |
| 1 | 4096 | durable_replay | 29.922 | 44480.675 | +155948.386% | [+141757.917%, +170651.100%] |
| 1 | 65536 | linear | 481.276 | 484.334 | +0.588% | [+0.446%, +0.730%] |
| 1 | 65536 | graph | 240.542 | 242.973 | +1.011% | [+0.949%, +1.064%] |
| 1 | 65536 | retry | 542.514 | 546.280 | +0.430% | [+0.001%, +0.735%] |
| 1 | 65536 | replay | 481.089 | 484.617 | +0.675% | [+0.598%, +0.777%] |
| 1 | 65536 | durable | 487.249 | 47835.717 | +8933.363% | [+8250.175%, +9672.447%] |
| 1 | 65536 | durable_replay | 487.372 | 51470.032 | +9065.757% | [+8429.790%, +9449.885%] |
| 1 | 1048576 | linear | 7670.266 | 7674.394 | +0.269% | [-0.061%, +0.690%] |
| 1 | 1048576 | graph | 3839.707 | 3844.066 | -0.201% | [-1.061%, +0.420%] |
| 1 | 1048576 | retry | 8639.306 | 8647.506 | +0.004% | [-0.069%, +0.095%] |
| 1 | 1048576 | replay | 7669.463 | 7677.236 | +0.319% | [+0.216%, +0.401%] |
| 1 | 1048576 | durable | 7849.181 | 56420.528 | +599.537% | [+560.602%, +655.666%] |
| 1 | 1048576 | durable_replay | 7835.859 | 58828.321 | +633.855% | [+588.372%, +682.343%] |
| 4 | 4096 | linear | 11.215 | 11.896 | +8.404% | [+6.103%, +12.515%] |
| 4 | 4096 | graph | 6.994 | 7.638 | +6.339% | [+2.230%, +9.432%] |
| 4 | 4096 | retry | 11.859 | 12.615 | +0.071% | [-9.566%, +10.734%] |
| 4 | 4096 | replay | 10.704 | 11.779 | +8.691% | [+2.881%, +15.410%] |
| 4 | 4096 | durable | 38.237 | 21357.416 | +54751.958% | [+49199.005%, +60700.267%] |
| 4 | 4096 | durable_replay | 27.673 | 22619.210 | +94220.783% | [+87254.388%, +102636.473%] |
| 4 | 65536 | linear | 125.261 | 126.467 | +6.101% | [+2.073%, +9.976%] |
| 4 | 65536 | graph | 63.973 | 64.773 | +4.526% | [-3.432%, +13.364%] |
| 4 | 65536 | retry | 139.899 | 141.182 | +8.151% | [+1.537%, +15.196%] |
| 4 | 65536 | replay | 124.932 | 125.992 | -4.528% | [-19.870%, +7.437%] |
| 4 | 65536 | durable | 270.488 | 22337.528 | +7976.533% | [+6766.897%, +8927.114%] |
| 4 | 65536 | durable_replay | 334.836 | 21883.576 | +7975.053% | [+7412.106%, +8623.378%] |
| 4 | 1048576 | linear | 1930.409 | 1932.415 | -2.376% | [-4.999%, +0.320%] |
| 4 | 1048576 | graph | 967.017 | 967.533 | +1.314% | [-3.589%, +5.111%] |
| 4 | 1048576 | retry | 2170.077 | 2171.449 | -1.620% | [-7.497%, +3.637%] |
| 4 | 1048576 | replay | 1929.390 | 1930.605 | -1.946% | [-5.283%, -0.135%] |
| 4 | 1048576 | durable | 2484.498 | 26199.516 | +969.284% | [+901.439%, +1053.463%] |
| 4 | 1048576 | durable_replay | 2618.441 | 22270.662 | +826.975% | [+720.535%, +947.222%] |

## Declared synthetic overall estimate

The default mix assigns equal workflow counts to all six cases. It weights measured costs before computing a ratio; it is an estimate from separately measured cases, not a measured interleaved mix. The expensive durable paths dominate this declared mix. It is not representative of an unspecified application. Use `--mix` to supply actual workload proportions and rerun with business task sizes and storage appropriate to that application.

| Workers | Kernel work per call | Equal-case weighted overhead estimate |
| ---: | ---: | ---: |
| 1 | 4096 | +57055.715% |
| 1 | 65536 | +3478.251% |
| 1 | 1048576 | +223.409% |
| 4 | 4096 | +39768.008% |
| 4 | 65536 | +4096.557% |
| 4 | 1048576 | +361.712% |

## Verification and evidence

[Raw data and metadata](verification-data/2026-10-31-application-workloads/) include every reference sample and unrounded summary. `verify.py` validates the complete profile grid and recomputes the summary, verifies source hashes against the working tree and checks qualification evidence. The excluded, incomplete pilot is preserved separately with its reason: it rebuilt the projection but retained business state in memory. It was stopped, corrected and replaced before the fixed campaign; none of its measurements enter these results. A complete pre-layout campaign is also retained separately: qualification required a declaration-only module-entry layout, so the implementation file was renamed and the full campaign repeated on the final executable. Its earlier samples are also excluded from the reference summary.

Two workload integration tests verify all six business paths and reject missing durable business-result evidence. Three Python tests check a known twofold-cost calculation and refuse missing/duplicate/unverified samples, substituted output checksums and invalid mix weights. Executor all-target/all-feature checks pass on stable Rust and Rust 1.85.0, alongside workspace strict Clippy, formatting and layer checks. These are scoped qualification of the new development benchmark, not a repeated full release gate. No production behavior is changed by adding this suite.
