//! Constructs an action from an owned semantic key outside measured regions.
#![allow(clippy::expect_used)]
use penelope_domain::{EffectKey, ProcessAction};
pub fn action(key: EffectKey) -> ProcessAction {
    ProcessAction {
        tenant_id: key.tenant_id,
        process_id: key.process_id,
        definition_id: key.definition_id,
        definition_version: key.definition_version,
        definition_digest: key.definition_digest,
        action_id: "act_fixture".parse().expect("valid fixture action ID"),
        step_id: key.step_id,
        attempt: key.attempt,
        kind: key.kind,
        payload_digest: key.payload_digest,
    }
}
