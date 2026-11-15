//! Shared pure model for the memory and timeline examples.
use penelope::machine::*;

pub struct Counter;
pub const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 1,
};
impl Machine for Counter {
    type State = u64;
    type Event = u64;
    type Effect = u64;
    type Error = MachineError;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, _: &u64) -> Result<(), MachineError> {
        Ok(())
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &u64,
        amount: &u64,
        context: &TransitionContext,
        intents: &mut S,
    ) -> Result<Transition<u64>, MachineError> {
        if *amount == 0 {
            return Ok(Transition::Unhandled);
        }
        let Some(next) = current.checked_add(*amount) else {
            return Ok(Transition::Rejected {
                reason: RejectCode::new(1),
            });
        };
        intents.stage(ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(context.event.bytes()),
                digest: penelope::core::task::Digest::of(&next.to_le_bytes()).0,
                payload_bytes: 8,
            },
            payload: next,
        })?;
        Ok(Transition::Accepted {
            next,
            transition: TransitionId::new(1),
        })
    }
}
