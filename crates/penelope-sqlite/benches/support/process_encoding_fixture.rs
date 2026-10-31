//! Fresh public commits; setup and durable verification stay outside measurement.
#[allow(clippy::unwrap_used)]
#[path = "../../../penelope-ports/tests/support/atomic_validation_fixture.rs"]
mod atomic;
pub(super) type Error = Box<dyn std::error::Error>;
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    num::NonZeroU16,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
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
pub(super) fn block_on<T>(future: impl Future<Output = T>) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or("deadline overflow")?;
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err("commit timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(5));
    }
}

pub(super) fn run(
    mut measure: impl FnMut(
        &SqliteProcessStore,
        &AtomicProcessCommit,
    ) -> Result<(AtomicProcessCommitReceipt, serde_json::Value), Error>,
) -> Result<(), Error> {
    atomic::commit(0).validate()?;
    for long in [false, true] {
        for outcomes in [1, 128] {
            for actions in [0, 128] {
                let directory = tempfile::tempdir()?;
                let path = directory.path().join("encoding.db");
                let store = SqliteProcessStore::open(
                    &path,
                    SqliteLimits::default(),
                    Arc::new(Clock),
                    10,
                    16,
                )?;
                let connection = rusqlite::Connection::open(&path)?;
                for sample in 0..10_u64 {
                    let mut batch = atomic::commit_with_identifiers(actions, long);
                    atomic::vary_payloads(&mut batch);
                    let input = batch.input.as_mut().ok_or("missing input")?;
                    input.input_id = format!("inp_{sample:04}").parse()?;
                    let first = batch.outcomes.first_mut().ok_or("missing outcome")?;
                    first.causation_id = CausationId::Input(input.input_id.clone());
                    let template = first.clone();
                    batch.outcomes.clear();
                    let base = sample.checked_mul(outcomes).ok_or("sequence overflow")?;
                    batch.expected_sequence = base;
                    for offset in 0..outcomes {
                        let mut row = template.clone();
                        row.sequence = base.checked_add(offset).ok_or("sequence overflow")?;
                        let padding = if long && offset % 2 == 0 {
                            "x".repeat(100)
                        } else {
                            String::new()
                        };
                        row.outcome_id = format!("out_{padding}{sample:04}_{offset:04}").parse()?;
                        batch.outcomes.push(row);
                    }
                    for (index, action) in batch.actions.iter_mut().enumerate() {
                        let padding = if long && index % 2 == 0 {
                            "x".repeat(100)
                        } else {
                            String::new()
                        };
                        action.action_id =
                            format!("act_{padding}{sample:04}_{index:04}").parse()?;
                        action.step_id = format!("stp_{padding}{sample:04}_{index:04}").parse()?;
                    }
                    batch.validate()?;
                    let checksum = ContentDigest::sha256(&serde_json::to_vec(&batch)?);
                    let (receipt, measurement) = measure(&store, &batch)?;
                    receipt.validate_for(&batch)?;
                    if receipt.duplicate_input {
                        return Err("fresh commit reported duplicate".into());
                    }
                    let request = OutcomeReplayRequest::new(
                        template.scope(),
                        base,
                        NonZeroU16::new(128).ok_or("zero page")?,
                    )?;
                    let page = block_on(store.read_outcomes(&request))??;
                    if page.outcomes != batch.outcomes || page.next_sequence.is_some() {
                        return Err("outcome replay differs".into());
                    }
                    for action in &batch.actions {
                        let bytes: Vec<u8> = connection.query_row(
                            "SELECT action FROM process_actions WHERE action_id=?1",
                            [action.action_id.as_str()],
                            |row| row.get(0),
                        )?;
                        if bytes != serde_json::to_vec(action)? {
                            return Err("action encoding differs".into());
                        }
                    }
                    if sample >= 2 {
                        println!(
                            "{}",
                            serde_json::json!({"long":long,"outcomes":outcomes,"actions":actions,"sample":sample,"checksum":checksum,"measurement":measurement})
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
