#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{ContentDigest, ProcessAction, ProcessActionKind, StepId};
use penelope_ports::CanonicalReconciliation;

fn identifier<T: TryFrom<&'static str>>(value: &'static str) -> T {
    match T::try_from(value) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(reconciliation) = serde_json::from_slice::<CanonicalReconciliation>(data) {
        let action_id = match &reconciliation {
            CanonicalReconciliation::Committed { event, .. } => &event.action_id,
            CanonicalReconciliation::NotCommitted { action_id, .. }
            | CanonicalReconciliation::Unknown { action_id, .. } => action_id,
        };
        let _ = reconciliation.validate_for(action_id);
        let scope = match &reconciliation {
            CanonicalReconciliation::Committed { scope, .. }
            | CanonicalReconciliation::NotCommitted { scope, .. }
            | CanonicalReconciliation::Unknown { scope, .. } => scope.clone(),
        };
        let action = ProcessAction::new(
            scope,
            action_id.clone(),
            identifier::<StepId>("stp_fuzz"),
            0,
            ProcessActionKind::CanonicalCommand,
            ContentDigest([0; 32]),
        );
        let _ = reconciliation.validate_for_action(&action);
    }
});
