//! Before/after hosted supervisor fixture; fixed in-memory ports, no test harness.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use penelope_core::task::{runtime::*, *};
use penelope_executor::task::TaskSupervisor;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

struct CountingAllocator;
static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static REALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static DEALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED_BYTES: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
// SAFETY: each operation forwards the original pointer/layout to System. The
// instrumentation uses only atomics and cannot allocate, unwind or recurse.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ENABLED.load(Ordering::SeqCst) {
            ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
            REQUESTED_BYTES.fetch_add(layout.size(), Ordering::SeqCst);
        }
        // SAFETY: caller supplies the GlobalAlloc layout contract unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if ENABLED.load(Ordering::SeqCst) {
            ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
            REQUESTED_BYTES.fetch_add(layout.size(), Ordering::SeqCst);
        }
        // SAFETY: caller supplies the GlobalAlloc layout contract unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if ENABLED.load(Ordering::SeqCst) {
            REALLOCATIONS.fetch_add(1, Ordering::SeqCst);
            REQUESTED_BYTES.fetch_add(size, Ordering::SeqCst);
        }
        // SAFETY: pointer, old layout and new size retain the caller's contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if ENABLED.load(Ordering::SeqCst) {
            DEALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: pointer/layout are forwarded to their original allocator.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[derive(Debug, PartialEq, Eq)]
struct Counts {
    allocations: usize,
    reallocations: usize,
    deallocations: usize,
    requested_bytes: usize,
}
fn start_counting() {
    ALLOCATIONS.store(0, Ordering::SeqCst);
    REALLOCATIONS.store(0, Ordering::SeqCst);
    DEALLOCATIONS.store(0, Ordering::SeqCst);
    REQUESTED_BYTES.store(0, Ordering::SeqCst);
    ENABLED.store(true, Ordering::SeqCst);
}
fn stop_counting() -> Counts {
    ENABLED.store(false, Ordering::SeqCst);
    Counts {
        allocations: ALLOCATIONS.load(Ordering::SeqCst),
        reallocations: REALLOCATIONS.load(Ordering::SeqCst),
        deallocations: DEALLOCATIONS.load(Ordering::SeqCst),
        requested_bytes: REQUESTED_BYTES.load(Ordering::SeqCst),
    }
}
#[derive(Clone, Copy)]
struct StoredBlob {
    reference: BlobRef,
    bytes: [u8; 8],
}
struct Journal {
    records: [Option<TaskRecord>; 32],
    count: usize,
    blobs: [Option<StoredBlob>; 8],
}
impl Journal {
    const fn new() -> Self {
        Self {
            records: [None; 32],
            count: 0,
            blobs: [None; 8],
        }
    }
}
impl TaskJournal for Journal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        let item = self
            .records
            .get(usize::try_from(sequence.get()).map_err(|_overflow| TaskPortError::Exhausted)?)
            .copied()
            .flatten();
        if item.is_some_and(|record| record.scope != scope) {
            return Err(TaskPortError::Corrupt);
        }
        Ok(item)
    }
    fn commit(
        &mut self,
        record: &TaskRecord,
        bytes: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        if usize::try_from(record.sequence.get()).ok() != Some(self.count) {
            return Err(TaskPortError::Conflict);
        }
        let destination = self
            .records
            .get_mut(self.count)
            .ok_or(TaskPortError::Exhausted)?;
        if let Some(data) = bytes {
            let reference = match record.event {
                TaskExecutionEvent::IntentCommitted { input, .. } => input,
                TaskExecutionEvent::CheckpointCommitted { checkpoint }
                | TaskExecutionEvent::CheckpointMigrated { checkpoint, .. } => checkpoint,
                TaskExecutionEvent::Completed { output }
                | TaskExecutionEvent::Reconciled {
                    result: Reconciliation::Completed { output, .. },
                } => output,
                TaskExecutionEvent::Started
                | TaskExecutionEvent::Restarted
                | TaskExecutionEvent::FaultObserved { .. }
                | TaskExecutionEvent::ReconciliationRequired
                | TaskExecutionEvent::Reconciled { .. }
                | TaskExecutionEvent::RecoveryPlanned { .. }
                | TaskExecutionEvent::RecoveryExhausted => return Err(TaskPortError::Corrupt),
            };
            reference
                .verify(data, reference.schema)
                .map_err(|_invalid| TaskPortError::Corrupt)?;
            if !self
                .blobs
                .iter()
                .flatten()
                .any(|entry| entry.reference == reference)
            {
                let mut stored = StoredBlob {
                    reference,
                    bytes: [0; 8],
                };
                stored
                    .bytes
                    .get_mut(..data.len())
                    .ok_or(TaskPortError::Exhausted)?
                    .copy_from_slice(data);
                *self
                    .blobs
                    .iter_mut()
                    .find(|entry| entry.is_none())
                    .ok_or(TaskPortError::Exhausted)? = Some(stored);
            }
        }
        *destination = Some(*record);
        self.count += 1;
        Ok(*record)
    }
    fn read_blob(
        &mut self,
        _scope: TaskScope,
        reference: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        let stored = self
            .blobs
            .iter()
            .flatten()
            .find(|entry| entry.reference == reference)
            .ok_or(TaskPortError::Corrupt)?;
        let length =
            usize::try_from(reference.bytes).map_err(|_overflow| TaskPortError::Exhausted)?;
        buffer
            .get_mut(..length)
            .ok_or(TaskPortError::Exhausted)?
            .copy_from_slice(stored.bytes.get(..length).ok_or(TaskPortError::Corrupt)?);
        Ok(length)
    }
}
#[derive(Default)]
struct Runtime {
    starts: u32,
    fences: u32,
    acks: u32,
    stage: u8,
    active: bool,
}
impl TaskRuntime for Runtime {
    fn start(&mut self, request: TaskStart<'_>) -> Result<(), TaskPortError> {
        if self.active || request.input != [3; 4] || request.input_schema != 1 {
            return Err(TaskPortError::Corrupt);
        }
        self.starts += 1;
        self.active = true;
        self.stage = u8::from(request.checkpoint.is_some());
        Ok(())
    }
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError> {
        if !self.active {
            return Err(TaskPortError::Runtime);
        }
        buffer
            .get_mut(..8)
            .ok_or(TaskPortError::Exhausted)?
            .copy_from_slice(&[9; 8]);
        Ok(TaskObservation {
            scope,
            generation,
            progress: if self.stage == 0 {
                RuntimeProgress::Checkpoint {
                    bytes: 8,
                    schema: 1,
                }
            } else {
                RuntimeProgress::Complete {
                    bytes: 8,
                    schema: 1,
                }
            },
        })
    }
    fn acknowledge_checkpoint(
        &mut self,
        _scope: TaskScope,
        _generation: TaskGeneration,
        _checkpoint: BlobRef,
    ) -> Result<(), TaskPortError> {
        self.acks += 1;
        self.stage = 1;
        Ok(())
    }
    fn fence(
        &mut self,
        _scope: TaskScope,
        _generation: TaskGeneration,
    ) -> Result<(), TaskPortError> {
        self.fences += 1;
        self.active = false;
        Ok(())
    }
}
const CAPACITY: usize = 65536;
type Control = TaskSupervisor<Journal, Runtime, CAPACITY>;
const fn definition(effects: EffectContract) -> TaskDefinition {
    TaskDefinition {
        code: Digest([1; 32]),
        checkpoint_schema: 1,
        effects,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromCheckpoint,
            max_attempts: 4,
            timeout_ticks: 50,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 8,
        },
        budget: ExecutionBudget {
            max_input_bytes: 4,
            max_output_bytes: 8,
            max_history_records: 32,
            max_checkpoint_commits: 4,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(1000),
        },
    }
}
fn scope(definition: TaskDefinition) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([2; 32])),
        task: TaskId::from_digest(Digest([4; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(&[3; 4])),
    }
}
fn lifecycle(effects: EffectContract) {
    let definition = definition(effects);
    let scope = scope(definition);
    let mut control = Control::restore(
        Journal::new(),
        Runtime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    let mut output = [0; 8];
    control.submit(&[3; 4], 1, TaskTick::ZERO).unwrap();
    control.advance(TaskTick::new(1)).unwrap();
    control.poll(TaskTick::new(2)).unwrap();
    let (journal, runtime) = control.into_parts();
    let mut recovered =
        Control::restore(journal, runtime, scope, definition, TaskTick::new(3)).unwrap();
    assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
    if effects == EffectContract::ReconcileRequired {
        recovered.advance(TaskTick::new(4)).unwrap();
        recovered
            .reconcile(
                scope,
                TaskGeneration::new(1),
                Reconciliation::Unknown {
                    evidence: Digest([5; 32]),
                },
                None,
                TaskTick::new(5),
            )
            .unwrap();
        assert!(!recovered.advance(TaskTick::new(6)).unwrap());
        let (unknown_journal, unknown_runtime) = recovered.into_parts();
        recovered = Control::restore(
            unknown_journal,
            unknown_runtime,
            scope,
            definition,
            TaskTick::new(7),
        )
        .unwrap();
        recovered
            .reconcile(
                scope,
                TaskGeneration::new(1),
                Reconciliation::RestartAuthorized {
                    evidence: Digest([6; 32]),
                },
                None,
                TaskTick::new(8),
            )
            .unwrap();
    }
    recovered.advance(TaskTick::new(9)).unwrap();
    recovered.advance(TaskTick::new(10)).unwrap();
    recovered.poll(TaskTick::new(11)).unwrap();
    assert_eq!(recovered.projection().status(), TaskStatus::Completed);
    let reference = recovered.projection().output().unwrap();
    let (mut finished_journal, finished_runtime) = recovered.into_parts();
    assert_eq!(
        finished_journal
            .read_blob(scope, reference, &mut output)
            .unwrap(),
        8
    );
    assert_eq!(output, [9; 8]);
    assert_eq!(finished_runtime.starts, 2);
    assert_eq!(finished_runtime.fences, 2);
    assert_eq!(finished_runtime.acks, 1);
}
#[inline(never)]
#[allow(clippy::missing_const_for_fn)]
fn stack_positive_control() {
    let mut bytes = [0_u8; 65_536];
    black_box(&mut bytes);
}
fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [path, repetitions] = arguments.as_slice() else {
        panic!("expected path and repetitions");
    };
    let repetitions: usize = repetitions.parse().unwrap();
    assert!(repetitions > 0);
    if path == "stack_baseline" {
        return;
    }
    if path == "stack_positive_control" {
        stack_positive_control();
        return;
    }
    let effects = match path.as_str() {
        "pure_checkpoint_recovery" => EffectContract::Pure,
        "effect_reconciliation_recovery" => EffectContract::ReconcileRequired,
        _ => panic!("unknown path"),
    };
    // Calibrate every allocator entry point outside the measured lifecycle interval.
    let small = Layout::from_size_align(8, 8).unwrap();
    let large = Layout::from_size_align(16, 8).unwrap();
    start_counting();
    // SAFETY: the two successful allocations use their matching layouts; the
    // first pointer is replaced by realloc's result and each is freed once.
    unsafe {
        let first = black_box(std::alloc::alloc(small));
        assert!(!first.is_null());
        let second = black_box(std::alloc::alloc_zeroed(small));
        assert!(!second.is_null());
        let resized = black_box(std::alloc::realloc(first, small, 16));
        assert!(!resized.is_null());
        std::alloc::dealloc(resized, large);
        std::alloc::dealloc(second, small);
    }
    assert_eq!(
        stop_counting(),
        Counts {
            allocations: 2,
            reallocations: 1,
            deallocations: 2,
            requested_bytes: 32
        }
    );
    start_counting();
    for _ in 0..repetitions {
        black_box(lifecycle)(black_box(effects));
    }
    let counts = stop_counting();
    assert_eq!(
        counts,
        Counts {
            allocations: 0,
            reallocations: 0,
            deallocations: 0,
            requested_bytes: 0
        }
    );
    println!(
        "{{\"capacity\":{CAPACITY},\"repetitions\":{repetitions},\"supervisor_bytes\":{},\"zero_sized_ports_supervisor_bytes\":{},\"allocations\":0,\"reallocations\":0,\"deallocations\":0,\"requested_bytes\":0}}",
        size_of::<Control>(),
        size_of::<TaskSupervisor<(), (), CAPACITY>>()
    );
}
