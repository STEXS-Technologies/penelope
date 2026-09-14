#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{ContentDigest, ProcessActionDtoV1, ProcessActionKindV1, StepId};
use penelope_ports::CanonicalReconciliationV1;

fn identifier<T: TryFrom<&'static str>>(value: &'static str) -> T {
    match T::try_from(value) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(reconciliation) = serde_json::from_slice::<CanonicalReconciliationV1>(data) {
        let action_id = match &reconciliation {
            CanonicalReconciliationV1::Committed { event, .. } => &event.action_id,
            CanonicalReconciliationV1::NotCommitted { action_id, .. }
            | CanonicalReconciliationV1::Unknown { action_id, .. } => action_id,
        };
        let _ = reconciliation.validate_for(action_id);
        let scope = match &reconciliation {
            CanonicalReconciliationV1::Committed { scope, .. }
            | CanonicalReconciliationV1::NotCommitted { scope, .. }
            | CanonicalReconciliationV1::Unknown { scope, .. } => scope.clone(),
        };
        let action = ProcessActionDtoV1::new(
            scope,
            action_id.clone(),
            identifier::<StepId>("stp_fuzz"),
            0,
            ProcessActionKindV1::CanonicalCommand,
            ContentDigest([0; 32]),
        );
        let _ = reconciliation.validate_for_action(&action);
    }
});
