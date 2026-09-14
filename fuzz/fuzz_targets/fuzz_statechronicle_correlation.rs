#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    ActionId, CanonicalEventDtoV1, ContentDigest, DefinitionId, DefinitionVersion, OperationId,
    ProcessId, ProcessScopeV1, ResourceId, TenantId,
};
use penelope_ports::AtomicProcessCommitV1;
use penelope_statechronicle::{
    CanonicalCommandExpectationV1, bind_verified_event_to_commit, verify_committed_event,
};

fn identifier<T: TryFrom<&'static str>>(value: &'static str) -> T {
    match T::try_from(value) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(expectation) = serde_json::from_slice::<CanonicalCommandExpectationV1>(data) {
        let _ = expectation.validate();
    }
    let expected = CanonicalCommandExpectationV1 {
        scope: ProcessScopeV1::new(
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
    if let (Ok(event), Ok(commit)) = (
        serde_json::from_slice::<CanonicalEventDtoV1>(data),
        serde_json::from_slice::<AtomicProcessCommitV1>(data),
    ) {
        if let Ok(verified) = verify_committed_event(&expected, event) {
            let _ = bind_verified_event_to_commit(commit, &verified);
        }
    }
});
