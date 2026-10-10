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
    fn read(&mut self, scope: TaskScope, seq: u32) -> Result<Option<TaskRecord>, TaskPortError> {
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
    fenced_through: u32,
    live: Option<u32>,
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
        if request.generation <= state.fenced_through
            || state.live.is_some_and(|live| live != request.generation)
        {
            return Err(TaskPortError::Conflict);
        }
        state.live = Some(request.generation);
        Ok(())
    }
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: u32,
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
        generation: u32,
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
    fn fence(&mut self, _scope: TaskScope, generation: u32) -> Result<(), TaskPortError> {
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
    let mut first =
        Supervisor::restore(journal.clone(), runtime.clone(), scope, definition, 0).unwrap();
    let mut second =
        Supervisor::restore(journal.clone(), runtime.clone(), scope, definition, 0).unwrap();
    first.submit(&input, 1, 0).unwrap();
    assert_eq!(
        second.submit(&input, 1, 0),
        Err(SupervisorError::Port(TaskPortError::Conflict))
    );
    assert_eq!(
        second.advance(1),
        Err(SupervisorError::ReconstructionRequired)
    );
    assert_eq!(journal.0.lock().unwrap().records.len(), 1);
    assert_eq!(runtime.state.lock().unwrap().live, None);
    first.advance(1).unwrap();
    assert_eq!(runtime.state.lock().unwrap().live, Some(1));
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
    let mut first = Supervisor::restore(journal.clone(), delayed, scope, definition, 0).unwrap();
    first.submit(&input, 1, 0).unwrap();
    let worker = std::thread::spawn(move || {
        assert_eq!(
            first.advance(1),
            Err(SupervisorError::Port(TaskPortError::Conflict))
        );
        assert_eq!(
            first.advance(4),
            Err(SupervisorError::ReconstructionRequired)
        );
    });
    authorized.wait(); // Started is durable, but runtime dispatch has not occurred.
    let mut recovered =
        Supervisor::restore(journal.clone(), runtime.clone(), scope, definition, 2).unwrap();
    assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
    recovered.advance(3).unwrap();
    recovered.advance(4).unwrap();
    assert_eq!(runtime.state.lock().unwrap().live, Some(2));
    resume.wait(); // The old request reaches the runtime after generation 2 starts.
    worker.join().unwrap();
    assert_eq!(runtime.state.lock().unwrap().live, Some(2));
    assert_eq!(runtime.state.lock().unwrap().fenced_through, 1);
    assert_eq!(journal.0.lock().unwrap().records.len(), 5);
}
