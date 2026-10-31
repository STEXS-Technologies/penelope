//! Worker-facing helper contracts, independent of the C fixture's encoder.
#![allow(clippy::indexing_slicing)]
use super::{definition, scope};
use penelope_core::task::{Digest, TaskGeneration, TaskTick, runtime::TaskPortError};
use penelope_process_runtime::worker::{
    WorkerMetadata, read_request, wait_checkpoint_ack, write_proposal,
};
use std::io::{self, Cursor, Write};

fn metadata(input: &[u8]) -> WorkerMetadata {
    let definition = definition(2);
    WorkerMetadata {
        scope: scope(definition, input),
        generation: TaskGeneration::new(1),
        definition,
        execution_deadline: TaskTick::new(51),
        input_schema: 1,
    }
}
fn digest(metadata: &WorkerMetadata) -> Digest {
    let mut bytes = b"penelope.linux-worker.scope.v1\0".to_vec();
    for item in [
        metadata.scope.tenant.digest(),
        metadata.scope.task.digest(),
        metadata.scope.definition.digest(),
        metadata.scope.input.digest(),
    ] {
        bytes.extend_from_slice(&item.0);
    }
    Digest::of(&bytes)
}
fn packet(metadata: &WorkerMetadata, input: &[u8], checkpoint: Option<&[u8]>) -> Vec<u8> {
    let encoded = serde_json::to_vec(metadata).unwrap();
    let mut bytes = b"PNLWRK01".to_vec();
    bytes.extend_from_slice(&digest(metadata).0);
    for number in [
        metadata.generation.get(),
        u32::try_from(encoded.len()).unwrap(),
        u32::try_from(input.len()).unwrap(),
        u32::try_from(checkpoint.map_or(0, <[u8]>::len)).unwrap(),
        u32::from(checkpoint.is_some()),
    ] {
        bytes.extend_from_slice(&number.to_be_bytes());
    }
    bytes.extend_from_slice(&encoded);
    bytes.extend_from_slice(input);
    if let Some(checkpoint) = checkpoint {
        bytes.extend_from_slice(checkpoint);
    }
    bytes
}
#[test]
fn start_packet_preserves_full_metadata_input_and_optional_checkpoint() {
    for checkpoint in [None, Some(b"".as_slice()), Some(b"checkpoint".as_slice())] {
        let metadata = metadata(b"input");
        let bytes = packet(&metadata, b"input", checkpoint);
        let mut reader = Cursor::new(&bytes);
        let request = read_request(&mut reader, 64, 64).unwrap();
        assert_eq!(request.metadata, metadata);
        assert_eq!(request.input, b"input");
        assert_eq!(request.checkpoint.as_deref(), checkpoint);
        assert_eq!(reader.position(), u64::try_from(bytes.len()).unwrap());
    }
}
#[test]
fn advertised_lengths_are_rejected_before_reading_or_allocating_payloads() {
    let bytes = packet(&metadata(b"input"), b"input", None);
    for (offset, length) in [
        (44, 8193_u32),
        (48, u32::MAX),
        (52, u32::MAX),
        (56, 2),
        (52, 1),
    ] {
        let mut header = bytes[..60].to_vec();
        header[offset..offset + 4].copy_from_slice(&length.to_be_bytes());
        let mut reader = Cursor::new(header);
        assert_eq!(
            read_request(&mut reader, 64, 64).err(),
            Some(TaskPortError::Exhausted)
        );
        assert_eq!(reader.position(), 60);
    }
}
#[test]
fn truncated_packets_and_substituted_envelopes_fail_closed() {
    let bytes = packet(&metadata(b"input"), b"input", Some(b"checkpoint"));
    for cut in 0..bytes.len() {
        assert!(read_request(&mut Cursor::new(&bytes[..cut]), 64, 64).is_err());
    }
    for offset in 0..44 {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert_eq!(
            read_request(&mut Cursor::new(changed), 64, 64).err(),
            Some(TaskPortError::Corrupt)
        );
    }
    let mut changed = bytes;
    changed[60] = b'!';
    assert_eq!(
        read_request(&mut Cursor::new(changed), 64, 64).err(),
        Some(TaskPortError::Corrupt)
    );
}
#[test]
fn metadata_definition_schema_input_and_definition_limits_are_enforced() {
    let original = metadata(b"input");
    for case in 0..6 {
        let mut changed = original.clone();
        match case {
            0 => changed.input_schema = 0,
            1 => changed.definition.code = Digest([9; 32]),
            2 => {
                changed.definition.recovery.max_attempts = 0;
                changed.scope.definition = changed.definition.fingerprint();
            }
            3 => {
                changed.definition.budget.max_input_bytes = 4;
                changed.scope.definition = changed.definition.fingerprint();
            }
            4 => {
                changed.definition.recovery.max_checkpoint_bytes = 2;
                changed.scope.definition = changed.definition.fingerprint();
            }
            _ => {
                changed.scope.input =
                    penelope_core::task::TaskInputDigest::from_digest(Digest([4; 32]))
            }
        }
        let bytes = packet(&changed, b"input", Some(b"checkpoint"));
        assert_eq!(
            read_request(&mut Cursor::new(bytes), 64, 64).err(),
            Some(TaskPortError::Corrupt)
        );
    }
}
struct BrokenPipe {
    flush_only: bool,
}
impl Write for BrokenPipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.flush_only {
            Ok(bytes.len())
        } else {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}
#[test]
fn proposals_bind_kind_scope_generation_schema_and_bytes_and_surface_pipe_errors() {
    let metadata = metadata(b"input");
    for checkpoint in [false, true] {
        let mut bytes = Vec::new();
        write_proposal(&mut bytes, &metadata, checkpoint, 7, b"result").unwrap();
        assert_eq!(&bytes[..8], b"PNLOBS01");
        assert_eq!(&bytes[8..40], &digest(&metadata).0);
        assert_eq!(&bytes[40..44], &1_u32.to_be_bytes());
        assert_eq!(bytes[44], if checkpoint { 2 } else { 1 });
        assert_eq!(&bytes[45..49], &6_u32.to_be_bytes());
        assert_eq!(&bytes[49..53], &7_u32.to_be_bytes());
        assert_eq!(&bytes[53..], b"result");
        for (schema, payload) in [(0, b"result".as_slice()), (1, [0; 65].as_slice())] {
            let mut rejected = Vec::new();
            assert_eq!(
                write_proposal(&mut rejected, &metadata, checkpoint, schema, payload),
                Err(TaskPortError::Exhausted)
            );
            assert!(rejected.is_empty());
        }
        for flush_only in [false, true] {
            assert_eq!(
                write_proposal(
                    &mut BrokenPipe { flush_only },
                    &metadata,
                    checkpoint,
                    1,
                    b"result"
                ),
                Err(TaskPortError::Runtime)
            );
        }
    }
}
#[test]
fn checkpoint_ack_requires_exact_magic_digest_and_complete_frame() {
    let digest = Digest::of(b"checkpoint");
    let mut bytes = b"PNLACK01".to_vec();
    bytes.extend_from_slice(&digest.0);
    assert_eq!(
        wait_checkpoint_ack(&mut Cursor::new(&bytes), digest),
        Ok(())
    );
    for cut in 0..bytes.len() {
        assert_eq!(
            wait_checkpoint_ack(&mut Cursor::new(&bytes[..cut]), digest),
            Err(TaskPortError::Runtime)
        );
    }
    for offset in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert_eq!(
            wait_checkpoint_ack(&mut Cursor::new(changed), digest),
            Err(TaskPortError::Corrupt)
        );
    }
}

#[test]
fn task_limits_and_schema_are_rejected_before_any_payload_read() {
    struct Guard {
        bytes: Cursor<Vec<u8>>,
        payload_at: u64,
        payload_touched: bool,
    }
    impl io::Read for Guard {
        fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
            if self.bytes.position() >= self.payload_at {
                self.payload_touched = true;
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            io::Read::read(&mut self.bytes, destination)
        }
    }
    for case in 0..3 {
        let mut metadata = metadata(b"input");
        match case {
            0 => metadata.definition.budget.max_input_bytes = 4,
            1 => metadata.definition.recovery.max_checkpoint_bytes = 2,
            _ => metadata.input_schema = 0,
        }
        metadata.scope.definition = metadata.definition.fingerprint();
        let bytes = packet(&metadata, b"input", Some(b"checkpoint"));
        let payload_at = 60 + u64::from(u32::from_be_bytes(bytes[44..48].try_into().unwrap()));
        let mut reader = Guard {
            bytes: Cursor::new(bytes),
            payload_at,
            payload_touched: false,
        };
        assert_eq!(
            read_request(&mut reader, 64, 64).err(),
            Some(TaskPortError::Corrupt)
        );
        assert!(!reader.payload_touched);
        assert_eq!(reader.bytes.position(), payload_at);
    }
}

#[test]
fn scope_hash_matches_legacy_wire_encoding_for_varied_scopes() {
    use penelope_core::task::{TaskDefinitionFingerprint, TaskId, TaskInputDigest, TaskTenantId};
    let mut metadata = metadata(b"input");
    for index in 0..10_000_u32 {
        let field = |tag: u8| {
            let mut bytes = index.to_be_bytes().to_vec();
            bytes.push(tag);
            Digest::of(&bytes)
        };
        metadata.scope.tenant = TaskTenantId::from_digest(field(0));
        metadata.scope.task = TaskId::from_digest(field(1));
        metadata.scope.definition = TaskDefinitionFingerprint::from_digest(field(2));
        metadata.scope.input = TaskInputDigest::from_digest(field(3));
        let mut bytes = Vec::new();
        write_proposal(&mut bytes, &metadata, false, 1, b"result").unwrap();
        assert_eq!(&bytes[8..40], &digest(&metadata).0);
    }
}
