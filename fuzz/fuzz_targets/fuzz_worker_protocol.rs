#![no_main]
// Exercise only the pipe protocol: no worker execution, filesystem or sandbox
// privileges are involved. Structured packets keep mutation past the magic gate.
#[path = "../../crates/penelope-executor/tests/support/tasks.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use penelope_core::task::{Digest, EffectContract, RecoveryMode, TaskGeneration, TaskTick};
use penelope_process_runtime::worker::{
    WorkerMetadata, read_request, wait_checkpoint_ack, write_proposal,
};
use std::io::Cursor;

fn scope_digest(metadata: &WorkerMetadata) -> Digest {
    let mut bytes = b"penelope.linux-worker.scope.v1\0".to_vec();
    for digest in [
        metadata.scope.tenant.digest(),
        metadata.scope.task.digest(),
        metadata.scope.definition.digest(),
        metadata.scope.input.digest(),
    ] {
        bytes.extend_from_slice(&digest.0);
    }
    Digest::of(&bytes)
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 16384 {
        return;
    }
    let _raw_request = read_request(&mut Cursor::new(data), 64, 64);
    let _raw_ack = wait_checkpoint_ack(&mut Cursor::new(data), Digest::of(data));
    let _metadata = serde_json::from_slice::<WorkerMetadata>(data);

    let input = &data[..data.len().min(64)];
    let checkpoint = &data[..data.len().min(32)];
    let has_checkpoint = data.first().copied().unwrap_or(0) & 1 != 0;
    let definition = support::definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let metadata = WorkerMetadata {
        scope: support::scope(definition, input),
        generation: TaskGeneration::new(1),
        definition,
        execution_deadline: TaskTick::new(50),
        input_schema: 1,
    };
    let encoded = serde_json::to_vec(&metadata).unwrap();
    let mut packet = b"PNLWRK01".to_vec();
    packet.extend_from_slice(&scope_digest(&metadata).0);
    for value in [
        metadata.generation.get(),
        encoded.len() as u32,
        input.len() as u32,
        if has_checkpoint {
            checkpoint.len() as u32
        } else {
            0
        },
        u32::from(has_checkpoint),
    ] {
        packet.extend_from_slice(&value.to_be_bytes());
    }
    packet.extend_from_slice(&encoded);
    packet.extend_from_slice(input);
    if has_checkpoint {
        packet.extend_from_slice(checkpoint);
    }
    let decoded = read_request(&mut Cursor::new(&packet), 64, 64).unwrap();
    assert_eq!(decoded.metadata, metadata);
    assert_eq!(decoded.input, input);
    assert_eq!(
        decoded.checkpoint.as_deref(),
        has_checkpoint.then_some(checkpoint)
    );

    // Every authenticated scope/generation byte is checked, including zero and
    // high-order generation bytes. A single-bit substitution must fail closed.
    let position = 8 + usize::from(data.first().copied().unwrap_or(0)) % 36;
    let mut substituted = packet.clone();
    substituted[position] ^= 1;
    assert!(read_request(&mut Cursor::new(&substituted), 64, 64).is_err());
    let cut = usize::from(data.get(1).copied().unwrap_or(0)) % packet.len();
    assert!(read_request(&mut Cursor::new(&packet[..cut]), 64, 64).is_err());
    // Mutate lengths, metadata and payload too, with a hard parser allocation
    // limit. Acceptance is allowed only when all parser invariants still hold.
    for (index, byte) in data.iter().take(64).enumerate() {
        let offset = (index * 257 + usize::from(*byte)) % packet.len();
        packet[offset] ^= *byte;
    }
    if let Ok(parsed) = read_request(&mut Cursor::new(&packet), 64, 64) {
        assert!(parsed.input.len() <= 64);
        assert_eq!(
            Digest::of(&parsed.input),
            parsed.metadata.scope.input.digest()
        );
        assert_eq!(
            parsed.metadata.scope.definition,
            parsed.metadata.definition.fingerprint()
        );
        assert_ne!(parsed.metadata.input_schema, 0);
        assert!(
            parsed
                .checkpoint
                .as_ref()
                .is_none_or(|bytes| bytes.len() <= 64)
        );
    }

    let schema = if data.first() == Some(&0) { 0 } else { 1 };
    let mut proposal = Vec::new();
    let result = write_proposal(&mut proposal, &metadata, has_checkpoint, schema, input);
    if schema == 0 {
        assert!(result.is_err());
        assert!(proposal.is_empty());
    } else {
        result.unwrap();
        assert_eq!(&proposal[..8], b"PNLOBS01");
        assert_eq!(&proposal[8..40], &scope_digest(&metadata).0);
        assert_eq!(&proposal[40..44], &metadata.generation.get().to_be_bytes());
        assert_eq!(proposal[44], if has_checkpoint { 2 } else { 1 });
        assert_eq!(&proposal[45..49], &(input.len() as u32).to_be_bytes());
        assert_eq!(&proposal[49..53], &schema.to_be_bytes());
        assert_eq!(proposal.len(), 53 + input.len());
        assert_eq!(&proposal[53..], input);
    }
    assert!(write_proposal(&mut Vec::new(), &metadata, false, 1, &[0; 65]).is_err());
    let digest = Digest::of(checkpoint);
    let mut ack = b"PNLACK01".to_vec();
    ack.extend_from_slice(&digest.0);
    assert!(wait_checkpoint_ack(&mut Cursor::new(&ack), digest).is_ok());
    ack[position % 40] ^= 1;
    assert!(wait_checkpoint_ack(&mut Cursor::new(&ack), digest).is_err());
});
