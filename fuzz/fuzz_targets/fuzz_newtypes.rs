#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_core::timeline::*;
use penelope_domain::{
    ActionId, CanonicalCommitId, CanonicalEventId, ContentDigest, DefinitionId, DefinitionVersion,
    ExternalReferenceId, InputId, LogicalTime, MigrationId, OperationId, OutcomeId, PrincipalId,
    ProcessId, ProcessInput, ProcessInputEnvelope, ProcessInputKind, ResourceId, ReviewId, StepId,
    TenantId,
};
use penelope_executor::engine::{RetryJitter, RetryJitterSeed};
use penelope_ports::{OutboxLeaseToken, task::*};
use serde::{Serialize, de::DeserializeOwned};
use std::{fmt::Debug, num::NonZeroU64, str::FromStr};

fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T) {
    let bytes = serde_json::to_vec(&value).unwrap();
    assert_eq!(serde_json::from_slice::<T>(&bytes).unwrap(), value);
}

fn wire<T: Serialize + DeserializeOwned + PartialEq + Debug>(data: &[u8]) {
    if let Ok(value) = serde_json::from_slice::<T>(data) {
        roundtrip(value);
    }
}

fn identifier<T: FromStr + Serialize + DeserializeOwned + PartialEq + Debug>(
    data: &[u8],
    prefix: &str,
) {
    if let Ok(text) = std::str::from_utf8(data) {
        let parsed = T::from_str(text);
        let decoded = serde_json::from_value::<T>(serde_json::Value::String(text.into()));
        assert_eq!(parsed.is_ok(), decoded.is_ok());
        if let Ok(value) = parsed {
            assert_eq!(value, decoded.unwrap());
            roundtrip(value);
        }
    }
    // Exercise accepted constructors on every input, including arbitrary bytes.
    let suffix: String = data
        .iter()
        .take(32)
        .map(|byte| char::from(b'a' + byte % 26))
        .collect();
    let valid = format!("{prefix}x{suffix}");
    roundtrip(T::from_str(&valid).ok().unwrap());
    wire::<T>(data);
}

fn timeline_digest<T>(digest: Digest, make: fn(Digest) -> T, get: fn(T) -> Digest) {
    assert_eq!(get(make(digest)), digest);
}

fn timeline_counter<T>(
    value: u64,
    make: fn(u64) -> T,
    get: fn(T) -> u64,
    next: fn(T) -> Option<T>,
) {
    assert_eq!(get(make(value)), value);
    assert_eq!(next(make(value)).map(get), value.checked_add(1));
}

fuzz_target!(|data: &[u8]| {
    identifier::<TenantId>(data, "tnt_");
    identifier::<ProcessId>(data, "prc_");
    identifier::<DefinitionId>(data, "def_");
    identifier::<DefinitionVersion>(data, "dfv_");
    identifier::<StepId>(data, "stp_");
    identifier::<InputId>(data, "inp_");
    identifier::<OutcomeId>(data, "out_");
    identifier::<ActionId>(data, "act_");
    identifier::<ReviewId>(data, "rev_");
    identifier::<PrincipalId>(data, "pri_");
    identifier::<CanonicalEventId>(data, "cev_");
    identifier::<CanonicalCommitId>(data, "cmt_");
    identifier::<ResourceId>(data, "res_");
    identifier::<OperationId>(data, "op_");
    identifier::<ExternalReferenceId>(data, "ext_");
    identifier::<MigrationId>(data, "mig_");

    let digest = Digest::of(data);
    let mut integer = [0; 8];
    for (slot, byte) in integer.iter_mut().zip(data) {
        *slot = *byte;
    }
    let value = u64::from_be_bytes(integer);
    let small = (value & u64::from(u32::MAX)) as u32;
    roundtrip(digest);
    roundtrip(ContentDigest::sha256(data));
    roundtrip(LogicalTime(value));
    roundtrip(OutboxLeaseToken::new(
        NonZeroU64::new(value.max(1)).unwrap(),
    ));
    roundtrip(RetryJitterSeed::from_digest(ContentDigest::sha256(data)));
    roundtrip(RetryJitter {
        max_jitter_millis: NonZeroU64::new(value.max(1)).unwrap(),
    });
    let envelope = ProcessInputEnvelope::new(ProcessInput::new(
        TenantId::from_str("tnt_fuzz").unwrap(),
        ProcessId::from_str("prc_fuzz").unwrap(),
        InputId::from_str("inp_fuzz").unwrap(),
        ProcessInputKind::CanonicalEvent,
        ContentDigest::sha256(data),
    ));
    envelope.validate().unwrap();
    roundtrip(envelope);
    roundtrip(TaskTenantId::from_digest(digest));
    roundtrip(TaskId::from_digest(digest));
    roundtrip(TaskDefinitionFingerprint::from_digest(digest));
    roundtrip(TaskInputDigest::from_digest(digest));
    roundtrip(TaskGeneration::new(small));
    roundtrip(TaskSequence::new(small));
    roundtrip(TaskTick::new(value));
    assert_eq!(TaskTenantId::from_digest(digest).digest(), digest);
    assert_eq!(TaskId::from_digest(digest).digest(), digest);
    assert_eq!(
        TaskDefinitionFingerprint::from_digest(digest).digest(),
        digest
    );
    assert_eq!(TaskInputDigest::from_digest(digest).digest(), digest);
    assert_eq!(TaskGeneration::new(small).get(), small);
    assert_eq!(TaskSequence::new(small).get(), small);
    assert_eq!(TaskTick::new(value).get(), value);

    timeline_digest::<TimelineTenantId>(
        digest,
        TimelineTenantId::from_digest,
        TimelineTenantId::digest,
    );
    timeline_digest::<TimelineId>(digest, TimelineId::from_digest, TimelineId::digest);
    timeline_digest::<TimelineModelId>(
        digest,
        TimelineModelId::from_digest,
        TimelineModelId::digest,
    );
    timeline_digest::<TimelineSchemaId>(
        digest,
        TimelineSchemaId::from_digest,
        TimelineSchemaId::digest,
    );
    timeline_digest::<TimelineInputId>(
        digest,
        TimelineInputId::from_digest,
        TimelineInputId::digest,
    );
    timeline_digest::<TimelineSnapshotId>(
        digest,
        TimelineSnapshotId::from_digest,
        TimelineSnapshotId::digest,
    );
    timeline_counter::<TimelineStep>(
        value,
        TimelineStep::new,
        TimelineStep::get,
        TimelineStep::checked_next,
    );
    timeline_counter::<TimelineRevision>(
        value,
        TimelineRevision::new,
        TimelineRevision::get,
        TimelineRevision::checked_next,
    );
    timeline_counter::<TimelineBranchId>(
        value,
        TimelineBranchId::new,
        TimelineBranchId::get,
        TimelineBranchId::checked_next,
    );
    timeline_counter::<InputRevision>(
        value,
        InputRevision::new,
        InputRevision::get,
        InputRevision::checked_next,
    );

    macro_rules! wires { ($($type:ty),+ $(,)?) => { $(wire::<$type>(data);)+ }; }
    wires!(
        ContentDigest,
        LogicalTime,
        OutboxLeaseToken,
        RetryJitterSeed,
        RetryJitter,
        ProcessInputEnvelope,
        Digest,
        TaskTenantId,
        TaskId,
        TaskDefinitionFingerprint,
        TaskInputDigest,
        TaskGeneration,
        TaskSequence,
        TaskTick
    );

    // Opaque lease tokens must never admit zero through deserialization.
    assert!(serde_json::from_slice::<OutboxLeaseToken>(b"0").is_err());
    assert!(serde_json::from_slice::<RetryJitter>(br#"{"max_jitter_millis":0}"#).is_err());
    assert_eq!(
        TaskTick::new(value).checked_add(1).map(TaskTick::get),
        value.checked_add(1)
    );
    assert_eq!(
        TaskTick::new(value).saturating_add(1).get(),
        value.saturating_add(1)
    );
    assert_eq!(
        TaskGeneration::new(small)
            .checked_add(1)
            .map(TaskGeneration::get),
        small.checked_add(1)
    );
    assert_eq!(
        TaskSequence::new(small).saturating_sub(1).get(),
        small.saturating_sub(1)
    );
});
