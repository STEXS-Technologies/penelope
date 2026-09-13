#![no_main]

use std::str::FromStr;

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    ActionId, CanonicalCommitId, CanonicalEventId, DefinitionId, DefinitionVersion, InputId,
    OperationId, OutcomeId, PrincipalId, ProcessId, ResourceId, ReviewId, StepId, TenantId,
};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = TenantId::from_str(text);
    let _ = ProcessId::from_str(text);
    let _ = DefinitionId::from_str(text);
    let _ = DefinitionVersion::from_str(text);
    let _ = StepId::from_str(text);
    let _ = InputId::from_str(text);
    let _ = OutcomeId::from_str(text);
    let _ = ActionId::from_str(text);
    let _ = ReviewId::from_str(text);
    let _ = PrincipalId::from_str(text);
    let _ = CanonicalEventId::from_str(text);
    let _ = CanonicalCommitId::from_str(text);
    let _ = ResourceId::from_str(text);
    let _ = OperationId::from_str(text);
});
