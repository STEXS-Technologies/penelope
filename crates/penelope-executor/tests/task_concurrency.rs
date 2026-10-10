//! Deterministic real-thread interleavings with shared CAS and persistent fences.
#![allow(clippy::unwrap_used)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::{runtime::*, *};
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use std::sync::{Arc, Barrier, Mutex};
use support::*;

#[derive(Clone, Default)]
struct SharedJournal(Arc<Mutex<MemoryJournal>>);
impl TaskJournal for SharedJournal {
    fn read(
        &mut self,
        scope: TaskScope,
        seq: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        self.0
            .lock()
            .map_err(|_poisoned| TaskPortError::Corrupt)?
            .read(scope, seq)
    }
    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        self.0
            .lock()
            .map_err(|_poisoned| TaskPortError::Corrupt)?
            .commit(record, blob)
    }
    fn read_blob(
        &mut self,
        scope: TaskScope,
        blob: BlobRef,
        dst: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        self.0
            .lock()
            .map_err(|_poisoned| TaskPortError::Corrupt)?
            .read_blob(scope, blob, dst)
    }
}

#[derive(Default)]
struct RuntimeState {
    fenced_through: TaskGeneration,
    live: Option<TaskGeneration>,
}
#[derive(Clone, Default)]
struct SharedRuntime {
    state: Arc<Mutex<RuntimeState>>,
    delay_start: Option<(Arc<Barrier>, Arc<Barrier>)>,
}
impl TaskRuntime for SharedRuntime {
    fn start(&mut self, request: TaskStart<'_>) -> Result<(), TaskPortError> {
        if let Some((authorized, resume)) = self.delay_start.take() {
            authorized.wait();
            resume.wait();
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_poisoned| TaskPortError::Corrupt)?;
        if request.generation <= TaskGeneration::new((state.fenced_through).get())
            || state.live.is_some_and(|live| live != request.generation)
        {
            return Err(TaskPortError::Conflict);
        }
        state.live = Some(TaskGeneration::new((request.generation).get()));
        Ok(())
    }
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        _buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError> {
        if self
            .state
            .lock()
            .map_err(|_poisoned| TaskPortError::Corrupt)?
            .live
            != Some(generation)
        {
            return Err(TaskPortError::Conflict);
        }
        Ok(TaskObservation {
            scope,
            generation,
            progress: RuntimeProgress::Pending,
        })
    }
    fn acknowledge_checkpoint(
        &mut self,
        _scope: TaskScope,
        generation: TaskGeneration,
        _checkpoint: BlobRef,
    ) -> Result<(), TaskPortError> {
        if self
            .state
            .lock()
            .map_err(|_poisoned| TaskPortError::Corrupt)?
            .live
            != Some(generation)
        {
            return Err(TaskPortError::Conflict);
        }
        Ok(())
    }
    fn fence(
        &mut self,
        _scope: TaskScope,
        generation: TaskGeneration,
    ) -> Result<(), TaskPortError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_poisoned| TaskPortError::Corrupt)?;
        state.fenced_through = state.fenced_through.max(generation);
        if state.live == Some(generation) {
            state.live = None;
        }
        Ok(())
    }
}
type Supervisor = TaskSupervisor<SharedJournal, SharedRuntime, 64>;

#[test]
fn competing_intents_cannot_dispatch_after_a_cas_conflict() {
    let definition = definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let input = [1; 64];
    let scope = scope(definition, &input);
    let journal = SharedJournal::default();
    let runtime = SharedRuntime::default();
    let mut first = Supervisor::restore(
        journal.clone(),
        runtime.clone(),
        scope,
        definition,
        TaskTick::new(0),
    )
    .unwrap();
    let mut second = Supervisor::restore(
        journal.clone(),
        runtime.clone(),
        scope,
        definition,
        TaskTick::new(0),
    )
    .unwrap();
    first.submit(&input, 1, TaskTick::new(0)).unwrap();
    assert_eq!(
        second.submit(&input, 1, TaskTick::new(0)),
        Err(SupervisorError::Port(TaskPortError::Conflict))
    );
    assert_eq!(
        second.advance(TaskTick::new(1)),
        Err(SupervisorError::ReconstructionRequired)
    );
    assert_eq!(journal.0.lock().unwrap().records.len(), 1);
    assert_eq!(runtime.state.lock().unwrap().live, None);
    first.advance(TaskTick::new(1)).unwrap();
    assert_eq!(
        runtime.state.lock().unwrap().live,
        Some(TaskGeneration::new(1))
    );
}

#[test]
fn delayed_dispatch_cannot_revive_a_generation_fenced_by_reconstruction() {
    let definition = definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let input = [1; 64];
    let scope = scope(definition, &input);
    let journal = SharedJournal::default();
    let runtime = SharedRuntime::default();
    let authorized = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let delayed = SharedRuntime {
        delay_start: Some((authorized.clone(), resume.clone())),
        ..runtime.clone()
    };
    let mut first = Supervisor::restore(
        journal.clone(),
        delayed,
        scope,
        definition,
        TaskTick::new(0),
    )
    .unwrap();
    first.submit(&input, 1, TaskTick::new(0)).unwrap();
    let worker = std::thread::spawn(move || {
        assert_eq!(
            first.advance(TaskTick::new(1)),
            Err(SupervisorError::Port(TaskPortError::Conflict))
        );
        assert_eq!(
            first.advance(TaskTick::new(4)),
            Err(SupervisorError::ReconstructionRequired)
        );
    });
    authorized.wait(); // Started is durable, but runtime dispatch has not occurred.
    let mut recovered = Supervisor::restore(
        journal.clone(),
        runtime.clone(),
        scope,
        definition,
        TaskTick::new(2),
    )
    .unwrap();
    assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
    recovered.advance(TaskTick::new(3)).unwrap();
    recovered.advance(TaskTick::new(4)).unwrap();
    assert_eq!(
        runtime.state.lock().unwrap().live,
        Some(TaskGeneration::new(2))
    );
    resume.wait(); // The old request reaches the runtime after generation 2 starts.
    worker.join().unwrap();
    assert_eq!(
        runtime.state.lock().unwrap().live,
        Some(TaskGeneration::new(2))
    );
    assert_eq!(
        runtime.state.lock().unwrap().fenced_through,
        TaskGeneration::new(1)
    );
    assert_eq!(journal.0.lock().unwrap().records.len(), 5);
}

#[test]
// Spawn every participant before joining; a lazy iterator would deadlock at
// the barrier while the other participants have not yet been spawned.
#[allow(clippy::needless_collect)]
fn many_competing_supervisors_commit_and_dispatch_exactly_one_execution() {
    const CONTENDERS: usize = 32;
    // Each round creates fresh state and lets the OS choose a different winner.
    for _round in 0..16 {
        let definition = definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
        let input = [1; 64];
        let scope = scope(definition, &input);
        let journal = SharedJournal::default();
        let runtime = SharedRuntime::default();
        let ready = Arc::new(Barrier::new(CONTENDERS));
        let workers: Vec<_> = (0..CONTENDERS)
            .map(|_| {
                let journal = journal.clone();
                let runtime = runtime.clone();
                let ready = ready.clone();
                std::thread::spawn(move || {
                    let mut supervisor =
                        Supervisor::restore(journal, runtime, scope, definition, TaskTick::ZERO)
                            .unwrap();
                    ready.wait(); // All projections saw the same empty history.
                    match supervisor.submit(&input, 1, TaskTick::ZERO) {
                        Ok(()) => Some(supervisor),
                        Err(error) => {
                            assert_eq!(error, SupervisorError::Port(TaskPortError::Conflict));
                            assert_eq!(
                                supervisor.advance(TaskTick::new(1)),
                                Err(SupervisorError::ReconstructionRequired)
                            );
                            None
                        }
                    }
                })
            })
            .collect();
        let mut winners: Vec<_> = workers
            .into_iter()
            .filter_map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(winners.len(), 1);
        assert_eq!(journal.0.lock().unwrap().records.len(), 1);
        assert_eq!(runtime.state.lock().unwrap().live, None);
        let mut winner = winners.pop().unwrap();
        winner.advance(TaskTick::new(1)).unwrap();
        assert_eq!(
            runtime.state.lock().unwrap().live,
            Some(TaskGeneration::new(1))
        );
        winner
            .observe(
                TaskObservation {
                    scope,
                    generation: TaskGeneration::new(1),
                    progress: RuntimeProgress::Complete {
                        bytes: 8,
                        schema: 1,
                    },
                },
                &64_u64.to_le_bytes(),
                TaskTick::new(2),
            )
            .unwrap();
        let committed = journal.0.lock().unwrap().records.clone();
        assert_eq!(committed.len(), 3);
        assert_eq!(
            replay(scope, definition, committed).unwrap(),
            *winner.projection()
        );
        assert_eq!(winner.projection().status(), TaskStatus::Completed);
        assert_eq!(runtime.state.lock().unwrap().live, None);
        assert_eq!(
            runtime.state.lock().unwrap().fenced_through,
            TaskGeneration::new(1)
        );
    }
}
