# Security response rehearsal

This drill covers saga workflows and recoverable tasks. It verifies that a
suspected dependency or authorization vulnerability can be contained without
emitting raw process/task payloads or replaying effects.
Run it at least quarterly and after a material supply-chain alert.

1. Record the affected commit, dependency advisory, build profile, and scope of
   potentially affected tenants. Do not paste payloads, credentials, or raw
   error messages into the issue.
2. Reproduce the report in an isolated checkout with locked dependencies. Run
   `cargo audit`, `cargo deny check advisories bans licenses sources`,
   `cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz`,
   and `./scripts/check-no-secrets.sh`.
3. Freeze release promotion. Treat all ambiguous external effects as unknown;
   reconcile by typed action/effect identity and escalate rather than retrying.
4. Compare pinned saga definition digests and ordered outcomes, or task
   definition fingerprints, generations and ordered history. Require explicit
   application-authorized migration for changed definitions/checkpoint schemas.
   Fence interrupted task generations before recovery; checkpoints require
   committed, integrity-checked bytes and compatible schemas.
5. Apply the patch on a branch, rerun the full CI gate and relevant fuzz target,
   and retain command output as the incident artifact.
6. Publish an advisory only after a fixed release or mitigation exists. Rotate
   credentials through the consuming deployment if exposure is possible, and
   document affected versions and recovery actions.

The rehearsal passes only when execution remains tenant-scoped, no raw payload
is logged, no stale lease or task generation advances state, and no unknown
effect is automatically retried. Task storage and runtime failures must require
reconstruction before further dispatch. Exercise `fuzz_task_supervisor`,
`fuzz_task_history` and `fuzz_task_checkpoint` for task recovery changes; adapter
drills must also verify durable journal integrity and persistent fencing.

See [the current contracts and validation](RELEASE.md). The
[2026-09-15 rehearsal](security-response-rehearsal-2026-09-15.md) records the
earlier saga-only revision and is historical evidence.
