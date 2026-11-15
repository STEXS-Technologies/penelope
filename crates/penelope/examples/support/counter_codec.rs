//! Canonical scalar payloads for the shared counter model.
use super::counter::{Counter, DEFINITION};
use penelope::{machine::*, storage::machine::MachineCodec};
use std::io::{Error, ErrorKind};

pub struct CounterCodec;
fn encode(value: u64, output: &mut [u8]) -> Result<usize, Error> {
    output
        .get_mut(..8)
        .ok_or(ErrorKind::WriteZero)?
        .copy_from_slice(&value.to_le_bytes());
    Ok(8)
}
fn decode(input: &[u8]) -> Result<u64, Error> {
    let bytes: [u8; 8] = input.try_into().map_err(|_size| ErrorKind::InvalidData)?;
    Ok(u64::from_le_bytes(bytes))
}
impl MachineCodec<Counter> for CounterCodec {
    type Error = Error;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn event_type(&self, _: &u64) -> MachineEventType {
        MachineEventType::new(1)
    }
    fn encode_state(&self, state: &u64, output: &mut [u8]) -> Result<usize, Error> {
        encode(*state, output)
    }
    fn decode_state(&self, input: &[u8]) -> Result<u64, Error> {
        decode(input)
    }
    fn encode_event(&self, event: &u64, output: &mut [u8]) -> Result<usize, Error> {
        encode(*event, output)
    }
    fn decode_event(&self, kind: MachineEventType, input: &[u8]) -> Result<u64, Error> {
        if kind != MachineEventType::new(1) {
            return Err(ErrorKind::InvalidData.into());
        }
        decode(input)
    }
    fn encode_effect(&self, effect: &u64, output: &mut [u8]) -> Result<usize, Error> {
        encode(*effect, output)
    }
    fn decode_effect(&self, input: &[u8]) -> Result<u64, Error> {
        decode(input)
    }
}
