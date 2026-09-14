#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_ports::{
    EffectDispatchRequestV1, ExternalEffectEvidenceV1, ProcessAuthorizationDecisionV1,
    ProcessAuthorizationRequestV1, TimerScheduleV1,
};

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<ProcessAuthorizationRequestV1>(data);
    let _ = serde_json::from_slice::<ProcessAuthorizationDecisionV1>(data);
    if let Ok(timer) = serde_json::from_slice::<TimerScheduleV1>(data) {
        let _ = timer.validate();
    }
    if let Ok(request) = serde_json::from_slice::<EffectDispatchRequestV1>(data) {
        let _ = request.validate();
    }
    if let Ok(evidence) = serde_json::from_slice::<ExternalEffectEvidenceV1>(data) {
        let _ = evidence;
    }
});
