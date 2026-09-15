#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_ports::{
    EffectDispatchRequest, ExternalEffectEvidence, ProcessAuthorizationDecision,
    ProcessAuthorizationRequest, TimerSchedule,
};

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<ProcessAuthorizationRequest>(data);
    let _ = serde_json::from_slice::<ProcessAuthorizationDecision>(data);
    if let Ok(timer) = serde_json::from_slice::<TimerSchedule>(data) {
        let _ = timer.validate();
    }
    if let Ok(request) = serde_json::from_slice::<EffectDispatchRequest>(data) {
        let _ = request.validate();
    }
    if let Ok(evidence) = serde_json::from_slice::<ExternalEffectEvidence>(data) {
        let _ = evidence;
    }
});
