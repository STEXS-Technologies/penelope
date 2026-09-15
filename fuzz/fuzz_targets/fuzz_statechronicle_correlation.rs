#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    ActionId, CanonicalEvent, ContentDigest, DefinitionId, DefinitionVersion, OperationId,
    ProcessId, ProcessScope, ResourceId, TenantId,
};
use penelope_statechronicle::{CanonicalCommandExpectation, verify_committed_event};

fn identifier<T: TryFrom<&'static str>>(value: &'static str) -> T {
    match T::try_from(value) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(expectation) = serde_json::from_slice::<CanonicalCommandExpectation>(data) {
        let _ = expectation.validate();
    }
    let expected = CanonicalCommandExpectation {
        scope: ProcessScope::new(
            identifier::<TenantId>("tnt_fuzz"),
            identifier::<ProcessId>("prc_fuzz"),
            identifier::<DefinitionId>("def_fuzz"),
            identifier::<DefinitionVersion>("dfv_one"),
            ContentDigest([0; 32]),
        ),
        action_id: identifier::<ActionId>("act_fuzz"),
        operation: identifier::<OperationId>("op_fuzz"),
        resource_ids: vec![identifier::<ResourceId>("res_fuzz")],
        expected_event_payload_digest: ContentDigest([0; 32]),
    };
    if let Ok(event) = serde_json::from_slice::<CanonicalEvent>(data) {
        let _ = verify_committed_event(&expected, event);
    }
});
