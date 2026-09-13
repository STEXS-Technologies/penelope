#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_ports::{
    ProcessAuthorizationDecisionV1, ProcessAuthorizationRequestV1, TimerScheduleV1,
};

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<ProcessAuthorizationRequestV1>(data);
    let _ = serde_json::from_slice::<ProcessAuthorizationDecisionV1>(data);
    let _ = serde_json::from_slice::<TimerScheduleV1>(data);
});
