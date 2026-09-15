# Security response rehearsal

This drill verifies that a suspected dependency or authorization vulnerability
can be contained without emitting raw process payloads or replaying effects.
Run it at least quarterly and after a material supply-chain alert.

1. Record the affected commit, dependency advisory, build profile, and scope of
   potentially affected tenants. Do not paste payloads, credentials, or raw
   error messages into the issue.
2. Reproduce the report in an isolated checkout with locked dependencies. Run
   `cargo audit --no-yanked`, `cargo deny check advisories bans licenses sources`,
   the workspace tests, and the secret scan.
3. Freeze release promotion. Treat all ambiguous external effects as unknown;
   reconcile by typed action/effect identity and escalate rather than retrying.
4. Compare the pinned definition digest and outcome-log sequence before and
   after the proposed patch. Reject any changed definition without an explicit
   migration record.
5. Apply the patch on a branch, rerun the full CI gate and relevant fuzz target,
   and retain command output as the incident artifact.
6. Publish an advisory only after a fixed release or mitigation exists. Rotate
   credentials through the consuming deployment if exposure is possible, and
   document affected versions and recovery actions.

The rehearsal passes only when the process remains tenant-scoped, no raw
payload is logged, no stale lease can acknowledge work, and no unknown effect
is automatically retried.
