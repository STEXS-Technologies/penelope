#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{ActionId, CanonicalEventDtoV1, OperationId, ResourceId, TenantId};
use penelope_statechronicle::{CanonicalCommandExpectationV1, verify_committed_event};

fn identifier<T: TryFrom<&'static str>>(value: &'static str) -> T {
    match T::try_from(value) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    let expected = CanonicalCommandExpectationV1 {
        tenant_id: identifier::<TenantId>("tnt_fuzz"),
        action_id: identifier::<ActionId>("act_fuzz"),
        operation: identifier::<OperationId>("op_fuzz"),
        resource_ids: vec![identifier::<ResourceId>("res_fuzz")],
    };
    if let Ok(event) = serde_json::from_slice::<CanonicalEventDtoV1>(data) {
        let _ = verify_committed_event(&expected, event);
    }
});
