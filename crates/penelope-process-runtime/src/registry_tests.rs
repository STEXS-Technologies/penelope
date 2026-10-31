//! Legacy-wire equivalence and fragmented manifest reads.
#![allow(clippy::unwrap_used)]
use super::*;
use penelope_core::task::{TaskDefinitionFingerprint, TaskId, TaskInputDigest, TaskTenantId};
use std::io::{self, Cursor, ErrorKind};

#[test]
fn fixed_header_matches_legacy_wire_for_varied_bindings() {
    let mut state = 17_u64;
    for _ in 0..10_000 {
        let mut digest = || {
            let mut bytes = [0; 32];
            for byte in &mut bytes {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state.to_le_bytes().first().copied().unwrap();
            }
            Digest(bytes)
        };
        let scope = TaskScope {
            tenant: TaskTenantId::from_digest(digest()),
            task: TaskId::from_digest(digest()),
            definition: TaskDefinitionFingerprint::from_digest(digest()),
            input: TaskInputDigest::from_digest(digest()),
        };
        let execution = digest();
        let boot = digest();
        let generation = TaskGeneration::new(u32::from_le_bytes(raw(&digest().0, 0).unwrap()));
        // Independent legacy scope encoding and growable header oracle.
        let mut scope_bytes = b"penelope.linux-worker.scope.v1\0".to_vec();
        for item in [
            scope.tenant.digest(),
            scope.task.digest(),
            scope.definition.digest(),
            scope.input.digest(),
        ] {
            scope_bytes.extend_from_slice(&item.0);
        }
        let mut legacy = b"PNLREG01".to_vec();
        legacy.extend_from_slice(&execution.0);
        legacy.extend_from_slice(&Digest::of(&scope_bytes).0);
        legacy.extend_from_slice(&generation.get().to_be_bytes());
        legacy.extend_from_slice(&scope.definition.digest().0);
        legacy.extend_from_slice(&boot.0);
        legacy.resize(172, 0);
        assert_eq!(
            header(scope, generation, execution, boot).as_slice(),
            legacy
        );
    }
}
struct Fragmented<'input> {
    bytes: &'input [u8],
    chunk: usize,
    interrupt: bool,
    fault_after: Option<usize>,
    consumed: usize,
}
impl Read for Fragmented<'_> {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(ErrorKind::Interrupted.into());
        }
        self.interrupt = true;
        if self.fault_after == Some(self.consumed) {
            return Err(ErrorKind::PermissionDenied.into());
        }
        let count = self.chunk.min(destination.len()).min(self.bytes.len());
        let (part, tail) = self.bytes.split_at(count);
        destination.get_mut(..count).unwrap().copy_from_slice(part);
        self.bytes = tail;
        self.consumed = self.consumed.saturating_add(count);
        Ok(count)
    }
}
#[test]
fn all_manifest_lengths_match_the_bounded_legacy_reader() {
    let bytes = [37; 350];
    for length in 0..=bytes.len() {
        let input = bytes.get(..length).unwrap();
        let mut legacy = Vec::new();
        Cursor::new(input)
            .take(173)
            .read_to_end(&mut legacy)
            .unwrap();
        let expected = if legacy.len() == 172 {
            Ok([37; 172])
        } else {
            Err(TaskPortError::Corrupt)
        };
        assert_eq!(
            read_manifest(Cursor::new(input)),
            expected,
            "length {length}"
        );
    }
}
#[test]
fn manifest_read_retries_interruptions_and_collects_short_reads() {
    for chunk in 1..=174 {
        for length in [0, 1, 171, 172, 173, 174] {
            let bytes = [29; 174];
            let reader = Fragmented {
                bytes: bytes.get(..length).unwrap(),
                chunk,
                interrupt: true,
                fault_after: None,
                consumed: 0,
            };
            let expected = if length == 172 {
                Ok([29; 172])
            } else {
                Err(TaskPortError::Corrupt)
            };
            assert_eq!(
                read_manifest(reader),
                expected,
                "chunk {chunk}, length {length}"
            );
        }
    }
}
#[test]
fn read_errors_before_or_after_a_complete_manifest_are_runtime_errors() {
    for fault_after in 0..=172 {
        let reader = Fragmented {
            bytes: &[11; 172],
            chunk: 1,
            interrupt: true,
            fault_after: Some(fault_after),
            consumed: 0,
        };
        assert_eq!(read_manifest(reader), Err(TaskPortError::Runtime));
    }
}
