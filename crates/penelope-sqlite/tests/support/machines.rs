//! Shared production-port fixtures for integration and process-death tests.
use super::{SqliteLimits, SqliteMachineStore};
use async_trait::async_trait;
use penelope_core::{machine::*, task::Digest};
use penelope_ports::machine::*;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
pub(super) const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 1,
};
pub(super) const SCOPE: MachineScope = MachineScope {
    tenant: MachineTenantId::from_bytes([1; 32]),
    instance: MachineId::from_bytes([2; 32]),
    definition: DEFINITION,
};
pub(super) const PAYLOADS: MachinePayloadLimits = MachinePayloadLimits {
    max_record_bytes: 4096,
    max_state_bytes: 8,
    max_event_bytes: 8,
    max_intent_bytes: 8,
    max_batch_bytes: 1024,
};
pub(super) const INTENTS: IntentLimits = IntentLimits {
    max_intents: 2,
    max_payload_bytes: 16,
};
pub(super) struct Model;
impl Machine for Model {
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
        state: &u64,
        event: &u64,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<u64>, MachineError> {
        let next = state.checked_add(*event).ok_or(MachineError::Capacity)?;
        sink.stage(ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(context.event.bytes()),
                digest: Digest::of(&next.to_le_bytes()).0,
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
pub(super) struct Codec;
pub(super) fn encode(value: u64, out: &mut [u8]) -> Result<usize, ()> {
    out.get_mut(..8)
        .ok_or(())?
        .copy_from_slice(&value.to_le_bytes());
    Ok(8)
}
pub(super) fn decode(bytes: &[u8]) -> Result<u64, ()> {
    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_invalid| ())?))
}
impl MachineCodec<Model> for Codec {
    type Error = ();
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn event_type(&self, _: &u64) -> MachineEventType {
        MachineEventType::new(1)
    }
    fn encode_state(&self, state: &u64, out: &mut [u8]) -> Result<usize, ()> {
        encode(*state, out)
    }
    fn decode_state(&self, bytes: &[u8]) -> Result<u64, ()> {
        decode(bytes)
    }
    fn encode_event(&self, event: &u64, out: &mut [u8]) -> Result<usize, ()> {
        encode(*event, out)
    }
    fn decode_event(&self, kind: MachineEventType, bytes: &[u8]) -> Result<u64, ()> {
        if kind != MachineEventType::new(1) {
            return Err(());
        }
        decode(bytes)
    }
    fn encode_effect(&self, effect: &u64, out: &mut [u8]) -> Result<usize, ()> {
        encode(*effect, out)
    }
    fn decode_effect(&self, bytes: &[u8]) -> Result<u64, ()> {
        decode(bytes)
    }
}
pub(super) struct Policy;
#[async_trait]
impl MachineAuthorizer for Policy {
    type Context = bool;
    async fn authorize(
        &self,
        authenticated: &bool,
        scope: MachineScope,
        _: MachineOperation,
    ) -> Result<(), MachineStoreError> {
        if *authenticated && scope.tenant == SCOPE.tenant {
            Ok(())
        } else {
            Err(MachineStoreError::Denied)
        }
    }
}
pub(super) struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
pub(super) fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(Instant::now() < deadline);
        std::thread::park_timeout(Duration::from_millis(2));
    }
}
pub(super) fn open(path: &std::path::Path) -> SqliteMachineStore<Policy, 2> {
    SqliteMachineStore::open(path, SqliteLimits::default(), PAYLOADS, Policy, 8).unwrap()
}
pub(super) fn genesis() -> Arc<PreparedMachineBootstrap> {
    Arc::new(prepare_machine_bootstrap(&Model, &Codec, SCOPE, &5, &mut [0; 8], PAYLOADS).unwrap())
}
pub(super) fn owner() -> MemoryMachine<Model, 2> {
    MemoryMachine::new(Model, SCOPE, 5, INTENTS).unwrap()
}
pub(super) fn batch(
    owner: &mut MemoryMachine<Model, 2>,
    event: u64,
    id: u8,
) -> Arc<MachineCommitBatch<2>> {
    let context = TransitionContext {
        expected: owner.read().0,
        event: MachineEventId::from_bytes([id; 32]),
        event_digest: Digest::of(&event.to_le_bytes()).0,
        logical_time: MachineLogicalTime::new(7),
    };
    let mut state = [0; 8];
    let mut encoded_event = [0; 8];
    let mut intent = [0; 8];
    let MachineCommitDecision::Prepared(ready) = prepare_machine_commit(
        owner,
        &MachineCommitInput {
            event: &event,
            context,
            codec: &Codec,
            causation: None,
        },
        &mut MachineCommitBuffers {
            state: &mut state,
            event: &mut encoded_event,
            intent: &mut intent,
        },
        PAYLOADS,
    )
    .unwrap() else {
        panic!("accepted fixture")
    };
    ready.batch()
}
pub(super) fn restore(store: &SqliteMachineStore<Policy, 2>) -> MemoryMachine<Model, 2> {
    block_on(store.load_head(&true, SCOPE))
        .unwrap()
        .unwrap()
        .restore(Model, &Codec, SCOPE, &mut [0; 8], INTENTS)
        .unwrap()
}
pub(super) const fn history_request() -> MachineHistoryRequest {
    MachineHistoryRequest {
        scope: SCOPE,
        after: MachineRevision::ZERO,
        through: MachineRevision::new(3),
        max_records: 2,
        max_bytes: 850,
    }
}
