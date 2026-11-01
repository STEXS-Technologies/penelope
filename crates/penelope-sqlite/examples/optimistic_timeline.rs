//! Optimistic invoice calculation and explicit finalized-intent -> v0.3 durable handoff.
#[path = "support/invoice_handoff.rs"]
mod invoice_handoff;
#[path = "support/invoice_model.rs"]
mod invoice_model;
use invoice_handoff::{Bridge, Clock, Destination, wait};
use invoice_model::Invoice;
use penelope_core::task::Digest;
use penelope_domain::*;
use penelope_ports::*;
use penelope_timeline::*;
use std::{
    num::NonZeroU16,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use thiserror::Error;

#[derive(Debug, Error)]
enum ExampleError {
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error(transparent)]
    Replay(#[from] ReplayError<TimelineError, TimelineError>),
    #[error(transparent)]
    Restore(#[from] RestoreError<TimelineError>),
    #[error(transparent)]
    Step(#[from] StepError<TimelineError, TimelineError, TimelineError>),
    #[error(transparent)]
    Handoff(#[from] HandoffError<PortError>),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
fn claim(scope: ProcessScope) -> Result<OutboxClaimRequest, PortError> {
    OutboxClaimRequest::new(
        scope,
        "pri_invoice_sender"
            .parse()
            .map_err(|_error| PortError::Invariant)?,
        NonZeroU16::MIN,
    )
    .map_err(|_error| PortError::Invariant)
}
// Assertions are the executable acceptance checks; errors retain the adapter taxonomy.
#[allow(clippy::panic_in_result_fn)]
fn scenario() -> Result<(), ExampleError> {
    let directory = tempfile::tempdir()?;
    let clock = Arc::new(Clock {
        now: AtomicU64::new(100),
    });
    let mut bridge = Bridge::open(directory.path(), Arc::clone(&clock), true)?;
    let mut destination = Destination::open(directory.path())?;
    let mut live = invoice_model::corrected_invoice()?;
    assert_eq!(live.handoff(&mut bridge, 1)?, 0);
    assert_eq!(bridge.attempts, 0);
    assert_eq!(bridge.commands.len()?, 0);
    assert!(destination.invoices()?.is_empty());
    // Independent application finality proof/authorization. No authority observation
    // or local promotion above was treated as permission for irreversible I/O.
    let head = live.read().0;
    live.finalize_effects(head, head.step())?;
    assert!(matches!(
        live.handoff(&mut bridge, 1),
        Err(HandoffError::Store(PortError::Unavailable))
    ));
    assert_eq!(live.eligible_effects().count(), 1); // Uncertain ACK retains exact intent.
    assert_eq!(bridge.commands.len()?, 1);
    drop(bridge);
    let mut reopened = Bridge::open(directory.path(), Arc::clone(&clock), false)?;
    assert_eq!(live.handoff(&mut reopened, 1)?, 1); // Authoritative exact retry, no second batch.
    assert_eq!(live.eligible_effects().count(), 0);
    assert_eq!(reopened.commands.len()?, 1);
    let descriptor = {
        // Obtain the committed action through a bounded receiver-scoped query. The
        // sample knows its admitted scope from the durable command binding.
        let database = rusqlite::Connection::open(directory.path().join("commands.db"))
            .map_err(|_error| PortError::Unavailable)?;
        let bytes: [u8; 32] = database
            .query_row("SELECT binding FROM commands", [], |row| {
                row.get_ref(0)?
                    .as_blob()
                    .map_err(|_error| rusqlite::Error::InvalidQuery)?
                    .try_into()
                    .map_err(|_error| rusqlite::Error::InvalidQuery)
            })
            .map_err(|_error| PortError::Invariant)?;
        Digest(bytes)
    };
    let scope = reopened.scope_for(descriptor)?;
    let request = claim(scope.clone())?;
    let leased = wait(reopened.store.claim(&request))?;
    assert_eq!(leased.len(), 1);
    let first = leased.first().ok_or(PortError::Invariant)?;
    assert!(!destination.deliver(&first.record.action, &reopened.commands)?);
    assert_eq!(
        destination.invoices()?,
        vec![Invoice {
            total: 44,
            units: 6
        }]
    );
    // Endpoint committed, but its ACK is lost: do not acknowledge the source lease.
    drop(destination);
    drop(reopened);
    clock.now.store(111, Ordering::SeqCst); // Injected lease time, not a timeline step.
    let redelivery = Bridge::open(directory.path(), Arc::clone(&clock), false)?;
    let mut recovered_destination = Destination::open(directory.path())?;
    let second_batch = wait(redelivery.store.claim(&request))?;
    assert_eq!(second_batch.len(), 1);
    let second = second_batch.first().ok_or(PortError::Invariant)?;
    assert_eq!(second.record.delivery_attempt, 1);
    assert_eq!(second.record.action, first.record.action);
    assert!(recovered_destination.deliver(&second.record.action, &redelivery.commands)?);
    assert_eq!(
        recovered_destination.invoices()?,
        vec![Invoice {
            total: 44,
            units: 6
        }]
    );
    wait(OutboxStore::acknowledge(
        &redelivery.store,
        second,
        LogicalTime(111),
    ))?;
    assert!(wait(redelivery.store.claim(&request))?.is_empty());
    let replay = OutcomeReplayRequest::new(scope, 0, NonZeroU16::MIN)
        .map_err(|_error| PortError::Invariant)?;
    let page = wait(redelivery.store.read_outcomes(&replay))?;
    page.validate().map_err(|_error| PortError::Invariant)?;
    assert_eq!(page.scope, replay.scope);
    assert_eq!(page.from_sequence, replay.from_sequence);
    assert_eq!(page.outcomes.len(), 1);
    assert_eq!(page.next_sequence, None);
    // Finalized history cannot be computationally corrected across this prefix.
    let current = live.read().0;
    let invalid = StateAnchor {
        scope: current.scope(),
        branch: current.branch(),
        step: TimelineStep::new(1),
        snapshot: TimelineSnapshotId::from_digest(Digest([0; 32])),
        digest: Digest([0; 32]),
    };
    assert!(matches!(
        ReplayPlan::prepare(
            current,
            current.revision(),
            Correction::State(invalid),
            &[],
            ReplayLimits {
                max_retained_inputs: 8,
                max_replay_steps: 8
            }
        ),
        Err(TimelineError::ReconciliationRequired)
    ));
    println!(
        "invoice: corrected_total=44 units=6 durable_outcomes=1 delivered_invoices=1 lost_acks=2"
    );
    Ok(())
}
fn main() -> Result<(), ExampleError> {
    scenario()
}
#[test]
fn delayed_authority_and_two_lost_acks_preserve_one_invoice() -> Result<(), ExampleError> {
    scenario()
}

#[test]
fn failed_atomic_commit_keeps_intent_and_only_a_bounded_orphan_payload() -> Result<(), ExampleError>
{
    let directory = tempfile::tempdir()?;
    let clock = Arc::new(Clock {
        now: AtomicU64::new(100),
    });
    let mut bridge = Bridge::open(directory.path(), clock, false)?;
    let connection = rusqlite::Connection::open(directory.path().join("process.db"))
        .map_err(|_error| PortError::Unavailable)?;
    connection
        .execute_batch(
            "CREATE TRIGGER fail_invoice BEFORE INSERT ON process_outcomes
        BEGIN SELECT RAISE(ABORT,'injected commit failure'); END;",
        )
        .map_err(|_error| PortError::Unavailable)?;
    let mut live = invoice_model::corrected_invoice()?;
    let current = live.read().0;
    live.finalize_effects(current, current.step())?;
    assert!(matches!(
        live.handoff(&mut bridge, 1),
        Err(HandoffError::Store(PortError::Conflict))
    ));
    assert_eq!(live.eligible_effects().count(), 1);
    assert_eq!(bridge.commands.len()?, 1);
    let count: i64 = connection
        .query_row("SELECT count(*) FROM process_outcomes", [], |row| {
            row.get(0)
        })
        .map_err(|_error| PortError::Unavailable)?;
    assert_eq!(count, 0);
    let actions: i64 = connection
        .query_row("SELECT count(*) FROM process_actions", [], |row| row.get(0))
        .map_err(|_error| PortError::Unavailable)?;
    assert_eq!(actions, 0);
    let inputs: i64 = connection
        .query_row("SELECT count(*) FROM process_inbox", [], |row| row.get(0))
        .map_err(|_error| PortError::Unavailable)?;
    assert_eq!(inputs, 0);
    connection
        .execute_batch("DROP TRIGGER fail_invoice")
        .map_err(|_error| PortError::Unavailable)?;
    assert_eq!(live.handoff(&mut bridge, 1)?, 1);
    assert_eq!(bridge.commands.len()?, 1);
    assert_eq!(live.eligible_effects().count(), 0);
    Ok(())
}
