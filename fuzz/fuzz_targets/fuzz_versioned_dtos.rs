#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    CanonicalCommandDtoV1, CanonicalEventDtoV1, ManualReviewDtoV1, ProcessActionDtoV1,
    ProcessDefinitionDtoV1, ProcessInputDtoV1, ProcessOutcomeDtoV1,
};

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<ProcessDefinitionDtoV1>(data);
    let _ = serde_json::from_slice::<ProcessInputDtoV1>(data);
    let _ = serde_json::from_slice::<ProcessOutcomeDtoV1>(data);
    let _ = serde_json::from_slice::<ProcessActionDtoV1>(data);
    let _ = serde_json::from_slice::<CanonicalCommandDtoV1>(data);
    let _ = serde_json::from_slice::<CanonicalEventDtoV1>(data);
    let _ = serde_json::from_slice::<ManualReviewDtoV1>(data);
});
