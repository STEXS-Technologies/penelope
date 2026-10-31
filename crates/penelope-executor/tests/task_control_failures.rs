//! Portable control must preserve ambiguity at adapter failure boundaries.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::{control::*, runtime::*, *};
use std::{cell::RefCell, rc::Rc};
use support::*;

#[derive(Clone)]
struct Journal {
    state: Rc<RefCell<MemoryJournal>>,
    read_error: Option<TaskPortError>,
    blob_damage: Option<BlobDamage>,
    blob_target: Option<Digest>,
}
#[derive(Clone, Copy)]
enum BlobDamage {
    Error,
    Length,
    Bytes,
}
impl Journal {
    fn new(state: MemoryJournal) -> Self {
        Self {
            state: Rc::new(RefCell::new(state)),
            read_error: None,
            blob_damage: None,
            blob_target: None,
        }
    }
}
impl TaskJournal for Journal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        if let Some(error) = self.read_error {
            return Err(error);
        }
        self.state.borrow_mut().read(scope, sequence)
    }
    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        self.state.borrow_mut().commit(record, blob)
    }
    fn read_blob(
        &mut self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        let damage = if self.blob_target.is_none_or(|target| target == blob.digest) {
            self.blob_damage
        } else {
            None
        };
        match damage {
            Some(BlobDamage::Error) => Err(TaskPortError::Unavailable),
            Some(BlobDamage::Length) => Ok(buffer.len().saturating_add(1)),
            Some(BlobDamage::Bytes) => {
                let len = self.state.borrow_mut().read_blob(scope, blob, buffer)?;
                buffer[0] ^= 1;
                Ok(len)
            }
            None => self.state.borrow_mut().read_blob(scope, blob, buffer),
        }
    }
}
#[derive(Clone, Default)]
struct Runtime(Rc<RefCell<SumRuntime>>);
impl TaskRuntime for Runtime {
    fn start(&mut self, request: TaskStart<'_>) -> Result<(), TaskPortError> {
        self.0.borrow_mut().start(request)
    }
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError> {
        self.0.borrow_mut().poll(scope, generation, buffer)
    }
    fn acknowledge_checkpoint(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        checkpoint: BlobRef,
    ) -> Result<(), TaskPortError> {
        self.0
            .borrow_mut()
            .acknowledge_checkpoint(scope, generation, checkpoint)
    }
    fn fence(&mut self, scope: TaskScope, generation: TaskGeneration) -> Result<(), TaskPortError> {
        self.0.borrow_mut().fence(scope, generation)
    }
}
type Control = TaskControl<Journal, Runtime>;
const INPUT: [u8; 8] = [1; 8];
fn fresh(effects: EffectContract) -> Control {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, effects);
    Control::restore(
        Journal::new(MemoryJournal::default()),
        Runtime::default(),
        scope(definition, &INPUT),
        definition,
        TaskTick::ZERO,
    )
    .unwrap()
}
fn advance(control: &mut Control, tick: u64) -> Result<bool, SupervisorError> {
    control.advance(
        TaskTick::new(tick),
        &mut TaskWorkspace {
            input: &mut [0; 64],
            output: &mut [0; 64],
            checkpoint: &mut [0; 64],
        },
    )
}
fn running(effects: EffectContract) -> Control {
    let mut control = fresh(effects);
    control.submit(&INPUT, 1, TaskTick::ZERO).unwrap();
    advance(&mut control, 1).unwrap();
    control
}

#[test]
fn restore_read_and_clock_failures_neither_fence_nor_append() {
    for read_failure in [false, true] {
        let control = running(EffectContract::Pure);
        let before = *control.projection();
        let (mut journal, runtime) = control.into_parts();
        let saved = journal.state.borrow().records.clone();
        if read_failure {
            journal.read_error = Some(TaskPortError::Unavailable);
        }
        let error = if read_failure {
            SupervisorError::Port(TaskPortError::Unavailable)
        } else {
            SupervisorError::Decision(TaskError::InvalidOrder)
        };
        assert!(
            matches!(Control::restore(journal.clone(), runtime.clone(), before.scope(), before.definition(), TaskTick::ZERO), Err(actual) if actual == error)
        );
        assert_eq!(journal.state.borrow().records, saved);
        assert_eq!(runtime.0.borrow().starts, 1);
        assert_eq!(runtime.0.borrow().fences, 0);
    }
}

#[test]
fn interruption_receipt_ambiguity_is_resolved_only_by_authoritative_replay() {
    for cut in [
        Cut::BeforeCommit,
        Cut::AfterCommit,
        Cut::WrongReceipt,
        Cut::StorageFull,
    ] {
        let control = running(EffectContract::ReconcileRequired);
        let before = *control.projection();
        let (journal, runtime) = control.into_parts();
        journal.state.borrow_mut().cut = Some(cut);
        let error = match cut {
            Cut::WrongReceipt => TaskPortError::Corrupt,
            Cut::StorageFull => TaskPortError::Exhausted,
            Cut::None | Cut::BeforeCommit | Cut::AfterCommit => TaskPortError::Unavailable,
        };
        assert!(
            matches!(Control::restore(journal.clone(), runtime.clone(), before.scope(), before.definition(), TaskTick::new(2)), Err(SupervisorError::Port(actual)) if actual == error)
        );
        assert_eq!(runtime.0.borrow().starts, 1);
        assert_eq!(runtime.0.borrow().fences, 1);
        let persisted = matches!(cut, Cut::AfterCommit | Cut::WrongReceipt);
        assert_eq!(
            journal.state.borrow().records.len(),
            if persisted { 3 } else { 2 }
        );
        let mut recovered = Control::restore(
            journal.clone(),
            runtime.clone(),
            before.scope(),
            before.definition(),
            TaskTick::new(3),
        )
        .unwrap();
        assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
        assert_eq!(journal.state.borrow().records.len(), 3);
        assert_eq!(runtime.0.borrow().fences, if persisted { 1 } else { 2 });
        advance(&mut recovered, 4).unwrap();
        assert_eq!(recovered.projection().status(), TaskStatus::Reconciling);
        assert!(recovered.projection().output().is_none());
        assert!(!advance(&mut recovered, 5).unwrap());
        assert_eq!(runtime.0.borrow().starts, 1);
    }
}

#[test]
fn corrupt_input_and_checkpoint_reads_poison_dispatch_before_user_code() {
    for checkpoint in [false, true] {
        for damage in [BlobDamage::Error, BlobDamage::Length, BlobDamage::Bytes] {
            let mut control = fresh(EffectContract::Pure);
            control.submit(&INPUT, 1, TaskTick::ZERO).unwrap();
            if checkpoint {
                advance(&mut control, 1).unwrap();
                control
                    .poll(
                        TaskTick::new(2),
                        &mut TaskWorkspace {
                            input: &mut [0; 64],
                            output: &mut [0; 64],
                            checkpoint: &mut [0; 64],
                        },
                    )
                    .unwrap();
            }
            let before = *control.projection();
            let (journal, runtime) = control.into_parts();
            let mut restored = Control::restore(
                journal.clone(),
                runtime.clone(),
                before.scope(),
                before.definition(),
                TaskTick::new(3),
            )
            .unwrap();
            if checkpoint {
                advance(&mut restored, 4).unwrap();
            }
            // Fail the selected read without altering the authoritative bytes.
            let (mut adapter, execution) = restored.into_parts();
            adapter.blob_target = if checkpoint {
                Some(before.checkpoint().unwrap().digest)
            } else {
                None
            };
            adapter.blob_damage = Some(damage);
            let mut dispatching = Control::restore(
                adapter,
                execution,
                before.scope(),
                before.definition(),
                TaskTick::new(4),
            )
            .unwrap();
            let error = advance(&mut dispatching, 5).unwrap_err();
            assert_eq!(
                error,
                match damage {
                    BlobDamage::Error => SupervisorError::Port(TaskPortError::Unavailable),
                    BlobDamage::Length => SupervisorError::Port(TaskPortError::Corrupt),
                    BlobDamage::Bytes => SupervisorError::Decision(TaskError::CorruptBlob),
                }
            );
            assert_eq!(
                advance(&mut dispatching, 6),
                Err(SupervisorError::ReconstructionRequired)
            );
            assert!(dispatching.projection().output().is_none());
            assert_eq!(runtime.0.borrow().starts, u32::from(checkpoint));
            assert!(matches!(
                journal.state.borrow().records.last().unwrap().event,
                TaskExecutionEvent::Started | TaskExecutionEvent::Restarted
            ));
        }
    }
}

#[test]
fn invalid_reconciliation_preserves_state_and_unknown_cannot_dispatch() {
    let control = running(EffectContract::ReconcileRequired);
    let running_projection = *control.projection();
    let (journal, runtime) = control.into_parts();
    let mut restored = Control::restore(
        journal.clone(),
        runtime.clone(),
        running_projection.scope(),
        running_projection.definition(),
        TaskTick::new(2),
    )
    .unwrap();
    advance(&mut restored, 3).unwrap();
    let before = *restored.projection();
    let records = journal.state.borrow().records.clone();
    let bytes = [7; 8];
    let reference = BlobRef {
        digest: Digest::of(&bytes),
        bytes: 8,
        schema: 1,
    };
    let complete = Reconciliation::Completed {
        output: reference,
        evidence: Digest([9; 32]),
    };
    let unknown = Reconciliation::Unknown {
        evidence: Digest([8; 32]),
    };
    for damage in [
        "scope",
        "generation",
        "missing",
        "bytes",
        "extra",
        "oversized",
        "clock",
    ] {
        let mut bound = before.scope();
        let mut generation = before.generation();
        let mut result = complete;
        let mut output: Option<&[u8]> = Some(&bytes);
        let mut tick = TaskTick::new(4);
        let error = match damage {
            "scope" => {
                bound.tenant = TaskTenantId::from_digest(Digest([6; 32]));
                TaskError::ScopeMismatch
            }
            "generation" => {
                generation = TaskGeneration::ZERO;
                TaskError::StaleGeneration
            }
            "missing" => {
                output = None;
                TaskError::CorruptBlob
            }
            "bytes" => {
                output = Some(&[8; 8]);
                TaskError::CorruptBlob
            }
            "extra" => {
                result = unknown;
                TaskError::CorruptBlob
            }
            "oversized" => {
                result = Reconciliation::Completed {
                    output: BlobRef {
                        bytes: 65,
                        ..reference
                    },
                    evidence: Digest([9; 32]),
                };
                TaskError::QuotaExhausted
            }
            _ => {
                tick = TaskTick::new(1);
                TaskError::InvalidOrder
            }
        };
        assert_eq!(
            restored.reconcile(bound, generation, result, output, tick),
            Err(SupervisorError::Decision(error)),
            "{damage}"
        );
        assert_eq!(restored.projection(), &before);
        assert_eq!(journal.state.borrow().records, records);
    }
    restored
        .reconcile(
            before.scope(),
            before.generation(),
            unknown,
            None,
            TaskTick::new(4),
        )
        .unwrap();
    for tick in 5..10 {
        assert!(!advance(&mut restored, tick).unwrap());
    }
    assert_eq!(restored.projection().status(), TaskStatus::Reconciling);
    assert!(restored.projection().output().is_none());
    assert_eq!(runtime.0.borrow().starts, 1);
    let (adapter, execution) = restored.into_parts();
    let mut replayed = Control::restore(
        adapter,
        execution,
        before.scope(),
        before.definition(),
        TaskTick::new(10),
    )
    .unwrap();
    assert!(!advance(&mut replayed, 11).unwrap());
    assert_eq!(replayed.projection().status(), TaskStatus::Reconciling);
    replayed
        .reconcile(
            before.scope(),
            before.generation(),
            complete,
            Some(&bytes),
            TaskTick::new(12),
        )
        .unwrap();
    assert_eq!(replayed.projection().status(), TaskStatus::Completed);
    assert_eq!(replayed.projection().output(), Some(reference));
    assert_eq!(journal.state.borrow().blobs[&reference.digest.0], bytes);
    assert_eq!(runtime.0.borrow().starts, 1);
}

#[test]
fn unconfirmed_quota_fence_poisoning_precedes_the_proposal_error() {
    for progress in [
        RuntimeProgress::Checkpoint {
            bytes: 65,
            schema: 1,
        },
        RuntimeProgress::Complete {
            bytes: 65,
            schema: 1,
        },
    ] {
        let initial = fresh(EffectContract::Pure);
        let projection = *initial.projection();
        let (journal, runtime) = initial.into_parts();
        let mut control = Control::restore(
            journal.clone(),
            runtime.clone(),
            projection.scope(),
            projection.definition(),
            TaskTick::ZERO,
        )
        .unwrap();
        control.submit(&INPUT, 1, TaskTick::ZERO).unwrap();
        advance(&mut control, 1).unwrap();
        let before = *control.projection();
        let records = journal.state.borrow().records.clone();
        runtime.0.borrow_mut().fence_failure = true;
        assert_eq!(
            control.observe(
                TaskObservation {
                    scope: before.scope(),
                    generation: before.generation(),
                    progress
                },
                &[],
                TaskTick::new(2)
            ),
            Err(SupervisorError::Port(TaskPortError::Conflict))
        );
        assert_eq!(control.projection(), &before);
        assert_eq!(journal.state.borrow().records, records);
        assert_eq!(
            advance(&mut control, 3),
            Err(SupervisorError::ReconstructionRequired)
        );
        assert_eq!(runtime.0.borrow().starts, 1);
        assert_eq!(runtime.0.borrow().fences, 0);
    }
}

#[test]
fn stale_observation_preserves_running_state_and_poll_at_deadline_records_timeout() {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let journal = Journal::new(MemoryJournal::default());
    let runtime = Runtime::default();
    let mut active = Control::restore(
        journal.clone(),
        runtime.clone(),
        scope(definition, &INPUT),
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    active.submit(&INPUT, 1, TaskTick::ZERO).unwrap();
    advance(&mut active, 1).unwrap();
    let before = *active.projection();
    let records = journal.state.borrow().records.clone();
    let observation = TaskObservation {
        scope: before.scope(),
        generation: before.generation(),
        progress: RuntimeProgress::Complete {
            bytes: 8,
            schema: 1,
        },
    };
    assert_eq!(
        active.observe(observation, &[8; 8], TaskTick::ZERO),
        Err(SupervisorError::Decision(TaskError::InvalidOrder))
    );
    assert_eq!(*active.projection(), before);
    assert_eq!(journal.state.borrow().records, records);
    assert_eq!(runtime.0.borrow().fences, 0);
    assert!(
        active
            .poll(
                before.execution_deadline(),
                &mut TaskWorkspace {
                    input: &mut [0; 64],
                    output: &mut [0; 64],
                    checkpoint: &mut [0; 64],
                }
            )
            .unwrap()
    );
    assert_eq!(active.projection().status(), TaskStatus::Faulted);
    assert!(active.projection().output().is_none());
    let (finished_journal, finished_runtime) = active.into_parts();
    assert_eq!(finished_runtime.0.borrow().starts, 1);
    assert_eq!(finished_runtime.0.borrow().fences, 1);
    assert!(matches!(
        finished_journal
            .state
            .borrow()
            .records
            .last()
            .unwrap()
            .event,
        TaskExecutionEvent::FaultObserved {
            fault: TaskFault::Timeout
        }
    ));
}

#[test]
fn zero_schema_live_output_is_fenced_without_commit_or_acknowledgement() {
    for checkpoint in [false, true] {
        let mut control = running(EffectContract::Pure);
        let before = *control.projection();
        let observation = TaskObservation {
            scope: before.scope(),
            generation: before.generation(),
            progress: if checkpoint {
                RuntimeProgress::Checkpoint {
                    bytes: 8,
                    schema: 0,
                }
            } else {
                RuntimeProgress::Complete {
                    bytes: 8,
                    schema: 0,
                }
            },
        };
        assert_eq!(
            control.observe(observation, &[8; 8], TaskTick::new(2)),
            Err(SupervisorError::Decision(TaskError::QuotaExhausted))
        );
        assert_eq!(*control.projection(), before);
        assert_eq!(
            advance(&mut control, 3),
            Err(SupervisorError::ReconstructionRequired)
        );
        let (journal, runtime) = control.into_parts();
        assert_eq!(runtime.0.borrow().fences, 1);
        assert_eq!(journal.state.borrow().records.len(), 2);
        assert_eq!(journal.state.borrow().blobs.len(), 1);
    }
}

#[test]
fn foreign_input_admission_leaves_no_durable_mutation_and_allows_correct_retry() {
    let initial = fresh(EffectContract::Pure);
    let before = *initial.projection();
    let (journal, runtime) = initial.into_parts();
    let mut control = Control::restore(
        journal.clone(),
        runtime.clone(),
        before.scope(),
        before.definition(),
        TaskTick::ZERO,
    )
    .unwrap();
    assert_eq!(
        control.submit(&[2; 8], 1, TaskTick::ZERO),
        Err(SupervisorError::Decision(TaskError::ScopeMismatch))
    );
    assert_eq!(*control.projection(), before);
    assert!(journal.state.borrow().records.is_empty());
    assert!(journal.state.borrow().blobs.is_empty());
    assert_eq!(runtime.0.borrow().starts, 0);
    assert_eq!(runtime.0.borrow().fences, 0);
    control.submit(&INPUT, 1, TaskTick::ZERO).unwrap();
    assert_eq!(
        control.advance_with_buffers(TaskTick::new(1), &mut [0; 64], &mut [0; 64]),
        Ok(true)
    );
    assert_eq!(journal.state.borrow().records.len(), 2);
    assert_eq!(journal.state.borrow().blobs.len(), 1);
    assert_eq!(runtime.0.borrow().starts, 1);
}
