//! Public replay latency at selected history sizes and page cursors.
//! Population uses typed public commits; validation is outside the timed region.
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteProcessStore};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
pub(super) type Error = Box<dyn std::error::Error>;
pub(super) struct Clock;
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
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return Ok(result);
        }
        if Instant::now() >= deadline {
            return Err("replay timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
pub(super) fn populate(
    store: &SqliteProcessStore,
    scope: &ProcessScope,
    count: u32,
) -> Result<Vec<ProcessOutcome>, Error> {
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        "inp_start".parse()?,
        ProcessInputKind::CanonicalEvent,
        ContentDigest([2; 32]),
    );
    let mut outcomes = Vec::new();
    for sequence in 0..count {
        outcomes.push(ProcessOutcome::new(
            scope.clone(),
            u64::from(sequence),
            ProcessOutcomeFact::new(
                format!("out_{sequence:06}").parse()?,
                CausationId::Input(input.input_id.clone()),
                OutcomeActor::System,
                LogicalTime(1),
                if sequence == 0 {
                    ProcessOutcomeKind::Started
                } else {
                    ProcessOutcomeKind::ActionPlanned
                },
                ContentDigest([3; 32]),
            ),
        ));
    }
    let mut start = 0_u64;
    for batch in outcomes.chunks(128) {
        let commit = AtomicProcessCommit::new(
            start,
            if start == 0 {
                Some(input.clone())
            } else {
                None
            },
            batch.to_vec(),
            Vec::new(),
        )?;
        block_on(ProcessStore::commit(store, &commit))??.validate_for(&commit)?;
        start = start
            .checked_add(u64::try_from(batch.len())?)
            .ok_or("sequence overflow")?;
    }
    Ok(outcomes)
}
pub(super) fn verify(
    page: &OutcomeReplayPage,
    request: &OutcomeReplayRequest,
    history: &[ProcessOutcome],
) -> Result<(), Error> {
    page.validate()?;
    let expected: Vec<_> = history
        .iter()
        .filter(|outcome| outcome.sequence >= request.from_sequence)
        .take(usize::from(request.limit.get()))
        .cloned()
        .collect();
    let next = request
        .from_sequence
        .checked_add(u64::try_from(expected.len())?)
        .ok_or("sequence overflow")?;
    let continuation = if next < u64::try_from(history.len())? {
        Some(next)
    } else {
        None
    };
    if page.scope != request.scope
        || page.from_sequence != request.from_sequence
        || page.outcomes != expected
        || page.next_sequence != continuation
    {
        return Err("incorrect replay page".into());
    }
    Ok(())
}

/// Optional maximum-width identities exercise heap-backed scope validation.
pub(super) fn scope() -> Result<ProcessScope, Error> {
    let long = match std::env::var("PENELOPE_REPLAY_LONG_SCOPE") {
        Ok(value) if value == "1" => true,
        Ok(_) => return Err("PENELOPE_REPLAY_LONG_SCOPE must be 1 when set".into()),
        Err(std::env::VarError::NotPresent) => false,
        Err(std::env::VarError::NotUnicode(_)) => return Err("non-Unicode scope option".into()),
    };
    let suffix = if long {
        "x".repeat(124)
    } else {
        "bench".to_owned()
    };
    Ok(ProcessScope::new(
        format!("tnt_{suffix}").parse()?,
        format!("prc_{suffix}").parse()?,
        format!("def_{suffix}").parse()?,
        format!("dfv_{}", if long { suffix.as_str() } else { "one" }).parse()?,
        ContentDigest([1; 32]),
    ))
}
