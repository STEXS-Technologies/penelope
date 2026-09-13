#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_ports::CanonicalReconciliationV1;

fuzz_target!(|data: &[u8]| {
    if let Ok(reconciliation) = serde_json::from_slice::<CanonicalReconciliationV1>(data) {
        let action_id = match &reconciliation {
            CanonicalReconciliationV1::Committed { event } => &event.action_id,
            CanonicalReconciliationV1::NotCommitted { action_id }
            | CanonicalReconciliationV1::Unknown { action_id } => action_id,
        };
        let _ = reconciliation.validate_for(action_id);
    }
});
