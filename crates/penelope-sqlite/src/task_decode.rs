//! Replay-only canonical scope comparison with the original JSON fallback.
use penelope_core::task::{runtime::TaskPortError, *};
use serde::Deserialize;
use std::io::Write as _;

const HEADER: &[u8] = b"{\"version\":1,\"scope\":";
// Apart from the replaced scope, these wire fields/types match TaskRecord.
// Exhaustive construction forces field review; differential tests check JSON.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedRecord {
    version: u16,
    #[serde(rename = "scope")]
    _scope: (),
    sequence: TaskSequence,
    generation: TaskGeneration,
    tick: TaskTick,
    event: TaskExecutionEvent,
}

fn encode_scope(scope: TaskScope, buffer: &mut [u8]) -> Result<usize, TaskPortError> {
    let capacity = buffer.len();
    let mut remaining = buffer;
    serde_json::to_writer(&mut remaining, &scope).map_err(|_invalid| TaskPortError::Corrupt)?;
    capacity
        .checked_sub(remaining.len())
        .ok_or(TaskPortError::Corrupt)
}

pub(crate) struct Decoder {
    scope: TaskScope,
    scope_bytes: [u8; 1024],
    scope_length: Option<usize>,
    record_bytes: [u8; 4096],
}
impl Decoder {
    pub(crate) const fn new(scope: TaskScope) -> Self {
        Self {
            scope,
            scope_bytes: [0; 1024],
            scope_length: None,
            record_bytes: [0; 4096],
        }
    }
    pub(crate) fn decode(
        &mut self,
        bytes: &[u8],
        checksum: &[u8],
        sequence: TaskSequence,
    ) -> Result<TaskRecord, TaskPortError> {
        if Digest::of(bytes).0.as_slice() != checksum {
            return Err(TaskPortError::Corrupt);
        }
        // Prepare an encoding only for a possible canonical record. The scope
        // and its encoding stay coupled in this decoder for the whole replay.
        let canonical = if let Some(rest) = bytes.strip_prefix(HEADER) {
            let encoded_scope_length = match self.scope_length {
                Some(length) => length,
                None => {
                    let length = encode_scope(self.scope, &mut self.scope_bytes)?;
                    self.scope_length = Some(length);
                    length
                }
            };
            let scope_json = self
                .scope_bytes
                .get(..encoded_scope_length)
                .ok_or(TaskPortError::Corrupt)?;
            rest.strip_prefix(scope_json).and_then(|tail| {
                HEADER
                    .len()
                    .checked_add(4)
                    .and_then(|length| length.checked_add(tail.len()))
                    .filter(|length| *length <= self.record_bytes.len())
                    .map(|length| (tail, length))
            })
        } else {
            None
        };
        let record = if let Some((tail, length)) = canonical {
            // Exact scope bytes were checked. Only that object is replaced by
            // null in scratch space; all remaining fields, duplicates and types
            // still pass through the strict record deserializer.
            let mut writer = self.record_bytes.as_mut_slice();
            writer
                .write_all(HEADER)
                .and_then(|()| writer.write_all(b"null"))
                .and_then(|()| writer.write_all(tail))
                .map_err(|_invalid| TaskPortError::Corrupt)?;
            let shortened = self
                .record_bytes
                .get(..length)
                .ok_or(TaskPortError::Corrupt)?;
            let parsed: ScopedRecord =
                serde_json::from_slice(shortened).map_err(|_invalid| TaskPortError::Corrupt)?;
            TaskRecord {
                version: parsed.version,
                scope: self.scope,
                sequence: parsed.sequence,
                generation: parsed.generation,
                tick: parsed.tick,
                event: parsed.event,
            }
        } else {
            // Reordered/escaped keys, pretty scopes, sequence-form JSON and
            // records exceeding scratch capacity retain the original decoder.
            serde_json::from_slice(bytes).map_err(|_invalid| TaskPortError::Corrupt)?
        };
        if record.scope != self.scope || record.sequence != sequence || record.version != 1 {
            return Err(TaskPortError::Corrupt);
        }
        Ok(record)
    }
}

#[cfg(test)]
#[path = "task_decode_tests.rs"]
mod tests;
