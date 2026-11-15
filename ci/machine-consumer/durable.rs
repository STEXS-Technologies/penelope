//! One model across memory, timeline and a local durable journal.
use async_trait::async_trait;
use penelope::{machine::*, storage::machine::*};
use penelope_sqlite::{SqliteLimits, SqliteMachineStore};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
#[path = "../../crates/penelope/examples/support/counter.rs"]
mod counter;
#[path = "../../crates/penelope/examples/support/counter_codec.rs"]
mod counter_codec;
use counter::{Counter, DEFINITION};
use counter_codec::CounterCodec;
const SCOPE: MachineScope = MachineScope {
    tenant: MachineTenantId::from_bytes([1; 32]),
    instance: MachineId::from_bytes([2; 32]),
    definition: DEFINITION,
};
const PAYLOADS: MachinePayloadLimits = MachinePayloadLimits {
    max_record_bytes: 4096,
    max_state_bytes: 8,
    max_event_bytes: 8,
    max_intent_bytes: 8,
    max_batch_bytes: 1024,
};
const INTENTS: IntentLimits = IntentLimits {
    max_intents: 4,
    max_payload_bytes: 32,
};
struct LocalOwner;
#[async_trait]
impl MachineAuthorizer for LocalOwner {
    // This local program selects its trusted owner. A service must derive this
    // context from its authenticated host request, never the submitted payload.
    type Context = MachineTenantId;
    async fn authorize(
        &self,
        owner: &MachineTenantId,
        scope: MachineScope,
        _operation: MachineOperation,
    ) -> Result<(), MachineStoreError> {
        if *owner == SCOPE.tenant && scope.tenant == *owner {
            Ok(())
        } else {
            Err(MachineStoreError::Denied)
        }
    }
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::park();
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("counter.db");
    let store =
        SqliteMachineStore::<_, 4>::open(&path, SqliteLimits::default(), PAYLOADS, LocalOwner, 8)?;
    let owner = SCOPE.tenant;
    let bootstrap =
        prepare_machine_bootstrap(&Counter, &CounterCodec, SCOPE, &0, &mut [0; 8], PAYLOADS)?;
    block_on(store.bootstrap(&owner, Arc::new(bootstrap)))?;
    let snapshot = block_on(store.load_head(&owner, SCOPE))?
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
    let mut machine: MemoryMachine<_, 4> =
        snapshot.restore(Counter, &CounterCodec, SCOPE, &mut [0; 8], INTENTS)?;
    let event = 7_u64;
    let context = TransitionContext {
        expected: machine.read().0,
        event: MachineEventId::from_bytes([4; 32]),
        event_digest: penelope::core::task::Digest::of(&event.to_le_bytes()).0,
        logical_time: MachineLogicalTime::new(1),
    };
    let mut state = [0; 8];
    let mut encoded_event = [0; 8];
    let mut intent = [0; 8];
    let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
        &mut machine,
        &MachineCommitInput {
            event: &event,
            context,
            codec: &CounterCodec,
            causation: None,
        },
        &mut MachineCommitBuffers {
            state: &mut state,
            event: &mut encoded_event,
            intent: &mut intent,
        },
        PAYLOADS,
    )?
    else {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    };
    let retained = prepared.batch();
    let receipt = block_on(store.commit(&owner, Arc::clone(&retained)))?;
    prepared.publish(receipt)?;
    // The journal retains exact durable intent bytes; release the local typed
    // cache after verified admission. This does not execute or acknowledge them.
    let _ = machine.take_intents();
    println!(
        "committed={} revision={}",
        machine.read().1,
        machine.read().0.revision.get()
    );
    drop(store);
    let reopened =
        SqliteMachineStore::<_, 4>::open(&path, SqliteLimits::default(), PAYLOADS, LocalOwner, 8)?;
    let duplicate = block_on(reopened.commit(&owner, retained))?;
    let stored = block_on(reopened.lookup(&owner, SCOPE, context.event))?
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
    if duplicate != stored.receipt() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    }
    let restored = stored.restore(Counter, &CounterCodec, SCOPE, &mut [0; 8], INTENTS)?;
    println!(
        "restored={} retained_intents={} duplicate={:?}",
        restored.read().1,
        stored.batch().intents().count(),
        duplicate.disposition
    );
    // Intent bytes remain journal evidence. Leasing, source authorization and
    // downstream acknowledgement/reconciliation must precede external execution.
    Ok(())
}
