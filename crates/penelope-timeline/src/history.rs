//! Fixed-capacity full-snapshot history with immutable branch lineage.
//!
//! Admissions and lookups scan configured slot/branch bounds, never total lifetime
//! history. Payloads are owned and immutable. The default store requires no heap;
//! `Clone` is required only by its optional full-state capture/restore convenience.
use crate::*;
use penelope_core::task::Digest;

/// Content identity and declared storage charge for a consumer payload.
/// Implementations must include all mutation-relevant bytes and report honest
/// sizes. Do not keep interior-mutable aliases to admitted values.
pub trait HistoryPayload {
    /// Identity recomputed at admission/read boundaries, never treated as authority.
    fn digest(&self) -> Digest;
    /// Storage charge checked before this store accepts or clones a payload.
    fn byte_len(&self) -> u64;
}
impl HistoryPayload for u64 {
    fn digest(&self) -> Digest {
        Digest::of(&self.to_le_bytes())
    }
    fn byte_len(&self) -> u64 {
        8
    }
}
impl HistoryPayload for i64 {
    fn digest(&self) -> Digest {
        Digest::of(&self.to_le_bytes())
    }
    fn byte_len(&self) -> u64 {
        8
    }
}
impl<const BYTES: usize> HistoryPayload for [u8; BYTES] {
    fn digest(&self) -> Digest {
        Digest::of(self)
    }
    fn byte_len(&self) -> u64 {
        u64::try_from(BYTES).unwrap_or(u64::MAX)
    }
}

/// Independent limits for input payloads, complete snapshots and aggregate charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryLimits {
    pub max_input_bytes: u64,
    pub max_snapshot_bytes: u64,
    pub max_total_bytes: u64,
}

/// Capture is separate from read-only restore; consumers need not implement Clone.
pub trait SnapshotCapture<State> {
    type Error;
    /// Captures a complete immutable state under the supplied scoped identity.
    /// # Errors
    /// Refuses incompatible, corrupt, duplicate or over-budget snapshots.
    fn capture(&mut self, anchor: StateAnchor, state: &State) -> Result<(), Self::Error>;
}

struct StoredInput<Input> {
    record: InputRecord,
    payload: Input,
    bytes: u64,
    canonical: bool,
}
struct StoredAnchor<State> {
    anchor: StateAnchor,
    state: State,
    bytes: u64,
    canonical: bool,
}
#[derive(Clone, Copy)]
struct Lineage {
    branch: TimelineBranchId,
    parent: Option<TimelineBranchId>,
    inherited_through: TimelineStep,
    admitted_through: TimelineStep,
    state_changed_from: TimelineStep,
    input_override: Option<InputRecord>,
    state_override: Option<StateAnchor>,
}

/// Owned bounded history; no eviction occurs implicitly on admission.
///
/// `expire_before` requires a complete floor anchor for every retained branch.
/// Read-only replay pins borrow this entire store, preventing mutation/eviction
/// while they live. Later inputs may be buffered elsewhere until the pin drops.
/// Root and ancestor branches cannot be retired while a child depends on them.
pub struct FixedHistory<
    State,
    Input,
    const INPUTS: usize,
    const ANCHORS: usize,
    const BRANCHES: usize,
> {
    scope: TimelineScope,
    inputs: [Option<StoredInput<Input>>; INPUTS],
    anchors: [Option<StoredAnchor<State>>; ANCHORS],
    branches: [Option<Lineage>; BRANCHES],
    floor: TimelineStep,
    last_branch: TimelineBranchId,
    bytes: u64,
    limits: HistoryLimits,
}
impl<
    State: HistoryPayload,
    Input: HistoryPayload,
    const INPUTS: usize,
    const ANCHORS: usize,
    const BRANCHES: usize,
> FixedHistory<State, Input, INPUTS, ANCHORS, BRANCHES>
{
    /// Creates one root branch at an authenticated complete baseline anchor.
    /// # Errors
    /// Refuses zero branch/anchor capacity, mismatched content and payload quotas.
    pub fn new(
        anchor: StateAnchor,
        state: State,
        limits: HistoryLimits,
    ) -> Result<Self, TimelineError> {
        let mut result = Self {
            scope: anchor.scope,
            inputs: core::array::from_fn(|_| None),
            anchors: core::array::from_fn(|_| None),
            branches: [None; BRANCHES],
            floor: anchor.step,
            last_branch: anchor.branch,
            bytes: 0,
            limits,
        };
        let branch = result.branches.first_mut().ok_or(TimelineError::Capacity)?;
        *branch = Some(Lineage {
            branch: anchor.branch,
            parent: None,
            inherited_through: anchor.step,
            admitted_through: anchor.step,
            state_changed_from: anchor.step,
            input_override: None,
            state_override: None,
        });
        result.insert_anchor(anchor, state, true)?;
        Ok(result)
    }
    /// Highest consumed branch identity, retained through materialization.
    /// Persist this together with the projection high-water mark; a live head alone
    /// cannot reconstruct abandoned reservation identities.
    #[must_use]
    pub const fn last_branch(&self) -> TimelineBranchId {
        self.last_branch
    }
    /// Oldest supported complete restore boundary; earlier history requires resync.
    #[must_use]
    pub const fn floor(&self) -> TimelineStep {
        self.floor
    }
    /// Aggregate admitted charge, not an allocator RSS measurement.
    #[must_use]
    pub const fn payload_bytes(&self) -> u64 {
        self.bytes
    }
    /// Stored input versions, including explicitly imported corrections.
    #[must_use]
    pub fn input_count(&self) -> usize {
        self.inputs.iter().flatten().count()
    }
    /// Stored complete snapshots, including unselected correction imports.
    #[must_use]
    pub fn anchor_count(&self) -> usize {
        self.anchors.iter().flatten().count()
    }
    /// Number of retained branch lineage records.
    #[must_use]
    pub fn branch_count(&self) -> usize {
        self.branches.iter().flatten().count()
    }
    /// Appends one immutable canonical input; it does not execute or publish state.
    /// # Errors
    /// Refuses gaps, scope/branch mismatch, changed content or count/byte exhaustion.
    pub fn append_input(
        &mut self,
        record: InputRecord,
        payload: Input,
    ) -> Result<(), TimelineError> {
        self.check_scope(record.scope)?;
        let lineage = self.lineage(record.branch)?;
        if record.step <= self.floor {
            return Err(TimelineError::HistoryUnavailable);
        }
        if record.step <= lineage.admitted_through {
            let original = self.source_input(record)?;
            if payload.digest() == record.digest && payload.byte_len() == original.bytes {
                return Ok(());
            }
            return Err(TimelineError::InputMismatch);
        }
        if record.step
            != lineage
                .admitted_through
                .checked_next()
                .ok_or(TimelineError::CounterExhausted)?
        {
            return Err(TimelineError::InputMismatch);
        }
        let bytes = self.check_payload(&payload, record.digest, self.limits.max_input_bytes)?;
        let total = self
            .bytes
            .checked_add(bytes)
            .ok_or(TimelineError::Capacity)?;
        if total > self.limits.max_total_bytes {
            return Err(TimelineError::Capacity);
        }
        let branch_slot = self
            .branches
            .iter_mut()
            .flatten()
            .find(|entry| entry.branch == record.branch)
            .ok_or(TimelineError::HistoryUnavailable)?;
        let slot = self
            .inputs
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(TimelineError::Capacity)?;
        *slot = Some(StoredInput {
            record,
            payload,
            bytes,
            canonical: true,
        });
        branch_slot.admitted_through = record.step;
        self.bytes = total;
        Ok(())
    }
    /// Imports an immutable corrected input without replacing canonical parent data.
    /// # Errors
    /// Refuses incompatible content/identity, an unknown branch and capacity exhaustion.
    pub fn import_input(
        &mut self,
        record: InputRecord,
        payload: Input,
    ) -> Result<(), TimelineError> {
        self.check_scope(record.scope)?;
        let lineage = self.lineage(record.branch)?;
        if record.step <= self.floor || record.step > lineage.admitted_through {
            return Err(TimelineError::HistoryUnavailable);
        }
        let bytes = self.check_payload(&payload, record.digest, self.limits.max_input_bytes)?;
        if let Some(old) = self.inputs.iter().flatten().find(|entry| {
            entry.record.branch == record.branch
                && entry.record.step == record.step
                && entry.record.revision == record.revision
        }) {
            if old.record == record && old.bytes == bytes {
                return Ok(());
            }
            return Err(TimelineError::InputMismatch);
        }
        let total = self
            .bytes
            .checked_add(bytes)
            .ok_or(TimelineError::Capacity)?;
        if total > self.limits.max_total_bytes {
            return Err(TimelineError::Capacity);
        }
        let slot = self
            .inputs
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(TimelineError::Capacity)?;
        *slot = Some(StoredInput {
            record,
            payload,
            bytes,
            canonical: false,
        });
        self.bytes = total;
        Ok(())
    }
    /// Stores an authenticated corrected snapshot without rewriting a canonical anchor.
    /// # Errors
    /// Refuses mismatched content, unknown scope/branch and resource exhaustion.
    pub fn import_anchor(
        &mut self,
        anchor: StateAnchor,
        state: State,
    ) -> Result<(), TimelineError> {
        self.insert_anchor(anchor, state, false)
    }
    /// Stores a complete canonical snapshot after its branch's inputs were admitted.
    /// # Errors
    /// Refuses an immutable-slot conflict, changed content or resource exhaustion.
    pub fn insert_snapshot(
        &mut self,
        anchor: StateAnchor,
        state: State,
    ) -> Result<(), TimelineError> {
        self.insert_anchor(anchor, state, true)
    }
    /// Records immutable child lineage before candidate publication. This does not
    /// select the candidate as live; failed promotion may retire this leaf later.
    /// # Errors
    /// Refuses branch reuse, missing imported corrections, absent parent data and capacity.
    pub fn fork(
        &mut self,
        plan: &ReplayPlan<'_>,
        reservation: BranchReservation,
    ) -> Result<(), TimelineError> {
        if reservation.expected() != plan.expected_head()
            || reservation.observation().step != plan.corrected_step()
            || reservation.observation().digest != plan.corrected_digest()
        {
            return Err(TimelineError::Conflict);
        }
        self.check_plan(plan)?;
        let parent = self.lineage(plan.expected_head().branch())?;
        if reservation.branch() <= self.last_branch {
            return Err(TimelineError::Conflict);
        }
        let replacement = plan.replacement_input();
        let slot = self
            .branches
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(TimelineError::Capacity)?;
        *slot = Some(Lineage {
            branch: reservation.branch(),
            parent: Some(parent.branch),
            inherited_through: plan.expected_head().step(),
            admitted_through: plan.expected_head().step(),
            state_changed_from: plan.corrected_step(),
            input_override: replacement,
            state_override: if replacement.is_none() {
                Some(plan.anchor())
            } else {
                None
            },
        });
        self.last_branch = reservation.branch();
        Ok(())
    }
    /// Resolves one canonical input on a branch, including immutable ancestor aliases.
    /// # Errors
    /// Returns `HistoryUnavailable` for missing/out-of-window input or ancestry.
    pub fn input_at(
        &self,
        branch: TimelineBranchId,
        step: TimelineStep,
    ) -> Result<InputRecord, TimelineError> {
        let mut record = self.resolve_input(branch, step)?.record;
        record.branch = branch;
        Ok(record)
    }
    /// Resolves a complete canonical anchor without borrowing corrected-away parent state.
    /// # Errors
    /// Refuses evicted, absent or invalidated anchors; the caller must resync or supply
    /// a complete captured anchor instead of silently replaying from the wrong state.
    pub fn snapshot_at(
        &self,
        branch: TimelineBranchId,
        step: TimelineStep,
    ) -> Result<StateAnchor, TimelineError> {
        let mut anchor = self.resolve_anchor(branch, step)?.anchor;
        anchor.branch = branch;
        Ok(anchor)
    }
    /// Copies one branch into a standalone bounded store, preserving the branch-ID
    /// high-water mark. Canonical inputs and every available complete anchor are
    /// resolved before copying; imports and unrelated branches are excluded.
    ///
    /// This leaves the source untouched. After its readers/sessions release their
    /// borrows, an owner may replace it with this store to reclaim old ancestry.
    /// The caller must retire other candidates before replacing their history.
    /// # Errors
    /// Refuses incomplete inputs, a missing floor anchor, corrupt content or quotas
    /// before cloning any payload. A consumer Clone panic cannot modify the source.
    pub fn materialize_branch(&self, branch: TimelineBranchId) -> Result<Self, TimelineError>
    where
        State: Clone,
        Input: Clone,
    {
        let through = self.lineage(branch)?.admitted_through;
        let count = through
            .get()
            .checked_sub(self.floor.get())
            .ok_or(TimelineError::HistoryUnavailable)?;
        let count = usize::try_from(count).map_err(|_error| TimelineError::Capacity)?;
        if count > INPUTS {
            return Err(TimelineError::Capacity);
        }
        let base = self.resolve_anchor(branch, self.floor)?;
        let mut bytes = 0_u64;
        let mut anchors = 0_usize;
        for offset in 0..=count {
            let step = self.materialized_step(offset)?;
            if offset != 0 {
                bytes = bytes
                    .checked_add(self.resolve_input(branch, step)?.bytes)
                    .ok_or(TimelineError::Capacity)?;
            }
            match self.resolve_anchor(branch, step) {
                Ok(anchor) => {
                    bytes = bytes
                        .checked_add(anchor.bytes)
                        .ok_or(TimelineError::Capacity)?;
                    anchors = anchors.checked_add(1).ok_or(TimelineError::Capacity)?;
                }
                Err(TimelineError::HistoryUnavailable) => {}
                Err(error) => return Err(error),
            }
        }
        if anchors > ANCHORS || bytes > self.limits.max_total_bytes {
            return Err(TimelineError::Capacity);
        }
        let mut anchor = base.anchor;
        anchor.branch = branch;
        let mut result = Self::new(anchor, base.state.clone(), self.limits)?;
        for offset in 1..=count {
            let step = self.materialized_step(offset)?;
            let source = self.resolve_input(branch, step)?;
            let mut record = source.record;
            record.branch = branch;
            result.append_input(record, source.payload.clone())?;
            match self.resolve_anchor(branch, step) {
                Ok(snapshot_source) => {
                    let mut snapshot_anchor = snapshot_source.anchor;
                    snapshot_anchor.branch = branch;
                    result.insert_snapshot(snapshot_anchor, snapshot_source.state.clone())?;
                }
                Err(TimelineError::HistoryUnavailable) => {}
                Err(error) => return Err(error),
            }
        }
        result.last_branch = self.last_branch;
        Ok(result)
    }
    fn materialized_step(&self, offset: usize) -> Result<TimelineStep, TimelineError> {
        let offset = u64::try_from(offset).map_err(|_error| TimelineError::Capacity)?;
        self.floor
            .get()
            .checked_add(offset)
            .map(TimelineStep::new)
            .ok_or(TimelineError::CounterExhausted)
    }
    /// Writes exact contiguous input descriptors into caller-owned scratch.
    /// # Errors
    /// Refuses a gap, future/out-of-window range or insufficient descriptor capacity.
    pub fn collect_inputs<'scratch>(
        &self,
        branch: TimelineBranchId,
        after: TimelineStep,
        through: TimelineStep,
        scratch: &'scratch mut [InputRecord],
    ) -> Result<&'scratch [InputRecord], TimelineError> {
        if after < self.floor || through < after {
            return Err(TimelineError::HistoryUnavailable);
        }
        let count = usize::try_from(
            through
                .get()
                .checked_sub(after.get())
                .ok_or(TimelineError::CounterExhausted)?,
        )
        .map_err(|_error| TimelineError::Capacity)?;
        let selected = scratch.get_mut(..count).ok_or(TimelineError::Capacity)?;
        let mut step = after;
        for slot in selected.iter_mut() {
            step = step.checked_next().ok_or(TimelineError::CounterExhausted)?;
            *slot = self.input_at(branch, step)?;
        }
        Ok(selected)
    }
    /// Borrows a replay store for the full session lifetime, statically preventing
    /// mutation, retirement and eviction while the session can still read inputs.
    /// # Errors
    /// Returns restoration/identity errors; live application state is unaffected.
    #[allow(clippy::type_complexity)]
    pub fn begin_replay<'history, 'inputs, Effect, const EFFECTS: usize>(
        &'history self,
        plan: ReplayPlan<'inputs>,
        reservation: BranchReservation,
        scratch: State,
        limits: EffectLimits,
    ) -> Result<
        PinnedReplaySession<'history, 'inputs, Self, State, Effect, EFFECTS>,
        RestoreError<TimelineError>,
    >
    where
        State: Clone,
    {
        self.check_plan(&plan)?;
        let session = ReplaySession::begin(plan, reservation, scratch, self, limits)?;
        Ok(session.pin_inputs(self))
    }
    /// Advances the retained floor only if every retained branch has a full anchor
    /// there. Existing read borrows/pinned sessions prevent this call in safe Rust.
    /// # Errors
    /// Refuses backward eviction or removal of the only required restore anchor.
    pub fn expire_before(&mut self, floor: TimelineStep) -> Result<(), TimelineError> {
        if floor < self.floor {
            return Err(TimelineError::HistoryUnavailable);
        }
        for branch in self.branches.iter().flatten() {
            if floor > branch.admitted_through {
                return Err(TimelineError::HistoryUnavailable);
            }
            self.snapshot_at(branch.branch, floor)?;
        }
        self.floor = floor;
        for slot in self.inputs.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|entry| entry.record.step <= floor)
            {
                let bytes = slot.as_ref().map_or(0, |entry| entry.bytes);
                let total = self
                    .bytes
                    .checked_sub(bytes)
                    .ok_or(TimelineError::Capacity)?;
                let retired = slot.take();
                self.bytes = total;
                drop(retired);
            }
        }
        for slot in self.anchors.iter_mut() {
            if slot.as_ref().is_some_and(|entry| entry.anchor.step < floor) {
                let bytes = slot.as_ref().map_or(0, |entry| entry.bytes);
                let total = self
                    .bytes
                    .checked_sub(bytes)
                    .ok_or(TimelineError::Capacity)?;
                let retired = slot.take();
                self.bytes = total;
                drop(retired);
            }
        }
        Ok(())
    }
    /// Retires an unselected leaf only. Ancestors, live branches and branches held
    /// by borrowed readers cannot be removed. Consumed branch IDs are never reused.
    /// # Errors
    /// Refuses a live/dependent/unknown branch or a mismatched live scope.
    /// If a consumer destructor unwinds, the leaf remains discoverable so cleanup
    /// can be retried; already removed payloads remain uncharged.
    pub fn retire_branch(
        &mut self,
        branch: TimelineBranchId,
        live: TimelineHead,
    ) -> Result<(), TimelineError> {
        self.check_scope(live.scope())?;
        self.lineage(live.branch())?;
        if branch == live.branch()
            || self
                .branches
                .iter()
                .flatten()
                .any(|entry| entry.parent == Some(branch))
        {
            return Err(TimelineError::Conflict);
        }
        let branch_slot = self
            .branches
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|entry| entry.branch == branch))
            .ok_or(TimelineError::HistoryUnavailable)?;
        for slot in self.inputs.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|entry| entry.record.branch == branch)
            {
                let bytes = slot.as_ref().map_or(0, |entry| entry.bytes);
                let total = self
                    .bytes
                    .checked_sub(bytes)
                    .ok_or(TimelineError::Capacity)?;
                let retired = slot.take();
                self.bytes = total;
                drop(retired);
            }
        }
        for slot in self.anchors.iter_mut() {
            if slot
                .as_ref()
                .is_some_and(|entry| entry.anchor.branch == branch)
            {
                let bytes = slot.as_ref().map_or(0, |entry| entry.bytes);
                let total = self
                    .bytes
                    .checked_sub(bytes)
                    .ok_or(TimelineError::Capacity)?;
                let retired = slot.take();
                self.bytes = total;
                drop(retired);
            }
        }
        // Keep the leaf discoverable until all consumer destructors finish. If
        // one unwinds, the caller can retry cleanup without stranding capacity.
        *branch_slot = None;
        Ok(())
    }
    fn check_plan(&self, plan: &ReplayPlan<'_>) -> Result<(), TimelineError> {
        self.check_scope(plan.expected_head().scope())?;
        let parent = self.lineage(plan.expected_head().branch())?;
        if parent.admitted_through < plan.expected_head().step() {
            return Err(TimelineError::HistoryUnavailable);
        }
        let replacement = plan.replacement_input();
        if let Some(record) = replacement {
            self.exact_input(record)?;
            if self.snapshot_at(parent.branch, plan.anchor().step)? != plan.anchor() {
                return Err(TimelineError::InputMismatch);
            }
        } else {
            match self.exact_anchor(plan.anchor()) {
                Ok(_) => {}
                Err(TimelineError::HistoryUnavailable) => {
                    if self.snapshot_at(parent.branch, plan.anchor().step)? != plan.anchor() {
                        return Err(TimelineError::InputMismatch);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        for offset in 0..plan.len() {
            let record = plan
                .input(offset)
                .ok_or(TimelineError::HistoryUnavailable)?;
            if Some(record) != replacement {
                self.source_input(record)?;
            }
        }
        Ok(())
    }
    fn check_scope(&self, scope: TimelineScope) -> Result<(), TimelineError> {
        if scope != self.scope {
            return Err(TimelineError::ScopeMismatch);
        }
        Ok(())
    }
    fn lineage(&self, branch: TimelineBranchId) -> Result<Lineage, TimelineError> {
        self.branches
            .iter()
            .flatten()
            .find(|entry| entry.branch == branch)
            .copied()
            .ok_or(TimelineError::HistoryUnavailable)
    }
    fn check_payload<T: HistoryPayload>(
        &self,
        payload: &T,
        digest: Digest,
        limit: u64,
    ) -> Result<u64, TimelineError> {
        let bytes = payload.byte_len();
        if bytes > limit {
            return Err(TimelineError::Capacity);
        }
        if payload.digest() != digest {
            return Err(TimelineError::InputMismatch);
        }
        Ok(bytes)
    }
    fn check_anchor_admission(
        &self,
        anchor: StateAnchor,
        state: &State,
        canonical: bool,
    ) -> Result<Option<(u64, u64)>, TimelineError> {
        self.check_scope(anchor.scope)?;
        let branch = self.lineage(anchor.branch)?;
        if anchor.step < self.floor || anchor.step > branch.admitted_through {
            return Err(TimelineError::HistoryUnavailable);
        }
        let bytes = self.check_payload(state, anchor.digest, self.limits.max_snapshot_bytes)?;
        if let Some(old) = self
            .anchors
            .iter()
            .flatten()
            .find(|entry| entry.anchor == anchor)
        {
            if old.bytes == bytes && (!canonical || old.canonical) {
                return Ok(None);
            }
            return Err(TimelineError::InputMismatch);
        }
        if canonical
            && self.anchors.iter().flatten().any(|entry| {
                entry.canonical
                    && entry.anchor.branch == anchor.branch
                    && entry.anchor.step == anchor.step
            })
        {
            return Err(TimelineError::InputMismatch);
        }
        let total = self
            .bytes
            .checked_add(bytes)
            .ok_or(TimelineError::Capacity)?;
        if total > self.limits.max_total_bytes {
            return Err(TimelineError::Capacity);
        }
        if self.anchors.iter().all(Option::is_some) {
            return Err(TimelineError::Capacity);
        }
        Ok(Some((bytes, total)))
    }
    fn insert_anchor(
        &mut self,
        anchor: StateAnchor,
        state: State,
        canonical: bool,
    ) -> Result<(), TimelineError> {
        let Some((bytes, total)) = self.check_anchor_admission(anchor, &state, canonical)? else {
            return Ok(());
        };
        let slot = self
            .anchors
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(TimelineError::Capacity)?;
        *slot = Some(StoredAnchor {
            anchor,
            state,
            bytes,
            canonical,
        });
        self.bytes = total;
        Ok(())
    }
    fn exact_input(&self, record: InputRecord) -> Result<&StoredInput<Input>, TimelineError> {
        self.check_scope(record.scope)?;
        if record.step <= self.floor {
            return Err(TimelineError::HistoryUnavailable);
        }
        // Slot position is only a hint: imports, eviction and branch-local
        // appends can place a different descriptor here. Verify the complete
        // record before using it, then fall back to an exact step-filtered scan.
        let entry = self
            .input_slot_hint(record.step)
            .filter(|entry| entry.record == record)
            .or_else(|| {
                self.inputs
                    .iter()
                    .flatten()
                    .find(|entry| entry.record.step == record.step && entry.record == record)
            })
            .ok_or(TimelineError::HistoryUnavailable)?;
        self.verify_input(entry)?;
        Ok(entry)
    }
    fn input_slot_hint(&self, step: TimelineStep) -> Option<&StoredInput<Input>> {
        let offset = step.get().checked_sub(self.floor.get())?.checked_sub(1)?;
        let index = usize::try_from(offset).ok()?;
        self.inputs.get(index)?.as_ref()
    }
    fn verify_input(&self, entry: &StoredInput<Input>) -> Result<(), TimelineError> {
        if self.check_payload(
            &entry.payload,
            entry.record.digest,
            self.limits.max_input_bytes,
        )? != entry.bytes
        {
            return Err(TimelineError::InputMismatch);
        }
        Ok(())
    }
    fn exact_anchor(&self, anchor: StateAnchor) -> Result<&StoredAnchor<State>, TimelineError> {
        self.check_scope(anchor.scope)?;
        if anchor.step < self.floor {
            return Err(TimelineError::HistoryUnavailable);
        }
        let entry = self
            .anchors
            .iter()
            .flatten()
            .find(|entry| entry.anchor.step == anchor.step && entry.anchor == anchor)
            .ok_or(TimelineError::HistoryUnavailable)?;
        self.verify_anchor(entry)?;
        Ok(entry)
    }
    fn verify_anchor(&self, entry: &StoredAnchor<State>) -> Result<(), TimelineError> {
        if self.check_payload(
            &entry.state,
            entry.anchor.digest,
            self.limits.max_snapshot_bytes,
        )? != entry.bytes
        {
            return Err(TimelineError::InputMismatch);
        }
        Ok(())
    }
    fn resolve_input(
        &self,
        mut branch: TimelineBranchId,
        step: TimelineStep,
    ) -> Result<&StoredInput<Input>, TimelineError> {
        if step <= self.floor {
            return Err(TimelineError::HistoryUnavailable);
        }
        for _depth in 0..BRANCHES {
            let lineage = self.lineage(branch)?;
            if step > lineage.admitted_through {
                return Err(TimelineError::HistoryUnavailable);
            }
            let canonical = |entry: &&StoredInput<Input>| {
                entry.record.step == step && entry.record.branch == branch && entry.canonical
            };
            // Forks inherit every input through the parent's admitted head.
            // append_input can only create child canonical entries after that
            // boundary; imports are noncanonical. Materialized roots have no
            // parent and must search their own complete canonical input range.
            if let Some(entry) = self.input_slot_hint(step).filter(canonical).or_else(|| {
                if lineage.parent.is_some() && step <= lineage.inherited_through {
                    None
                } else {
                    self.inputs.iter().flatten().find(canonical)
                }
            }) {
                self.verify_input(entry)?;
                return Ok(entry);
            }
            if let Some(record) = lineage.input_override.filter(|record| record.step == step) {
                return self.exact_input(record);
            }
            if step > lineage.inherited_through {
                return Err(TimelineError::HistoryUnavailable);
            }
            branch = lineage.parent.ok_or(TimelineError::HistoryUnavailable)?;
        }
        Err(TimelineError::HistoryUnavailable)
    }
    fn source_input(&self, record: InputRecord) -> Result<&StoredInput<Input>, TimelineError> {
        self.check_scope(record.scope)?;
        let source = self.resolve_input(record.branch, record.step)?;
        let mut alias = source.record;
        alias.branch = record.branch;
        if alias != record {
            return Err(TimelineError::InputMismatch);
        }
        Ok(source)
    }
    fn resolve_anchor(
        &self,
        mut branch: TimelineBranchId,
        step: TimelineStep,
    ) -> Result<&StoredAnchor<State>, TimelineError> {
        if step < self.floor {
            return Err(TimelineError::HistoryUnavailable);
        }
        for _depth in 0..BRANCHES {
            let lineage = self.lineage(branch)?;
            if step > lineage.admitted_through {
                return Err(TimelineError::HistoryUnavailable);
            }
            if let Some(entry) = self.anchors.iter().flatten().find(|entry| {
                entry.anchor.step == step && entry.anchor.branch == branch && entry.canonical
            }) {
                // The canonical search already selected the exact stored entry.
                // Recheck its content/charge without performing the same scan twice.
                self.verify_anchor(entry)?;
                return Ok(entry);
            }
            if let Some(anchor) = lineage.state_override.filter(|anchor| anchor.step == step) {
                return self.exact_anchor(anchor);
            }
            if step >= lineage.state_changed_from {
                return Err(TimelineError::HistoryUnavailable);
            }
            branch = lineage.parent.ok_or(TimelineError::HistoryUnavailable)?;
        }
        Err(TimelineError::HistoryUnavailable)
    }
}
impl<
    State: HistoryPayload + Clone,
    Input: HistoryPayload,
    const INPUTS: usize,
    const ANCHORS: usize,
    const BRANCHES: usize,
> SnapshotStore<State> for FixedHistory<State, Input, INPUTS, ANCHORS, BRANCHES>
{
    type Error = TimelineError;
    fn restore(
        &self,
        anchor: StateAnchor,
        scratch: &mut State,
    ) -> Result<StateAnchor, TimelineError> {
        self.check_scope(anchor.scope)?;
        // Explicit correction imports may be restored before a child is reserved.
        let source = match self.exact_anchor(anchor) {
            Ok(source) => source,
            Err(TimelineError::HistoryUnavailable) => {
                let source = self.resolve_anchor(anchor.branch, anchor.step)?;
                let mut alias = source.anchor;
                alias.branch = anchor.branch;
                if alias != anchor {
                    return Err(TimelineError::InputMismatch);
                }
                source
            }
            Err(error) => return Err(error),
        };
        scratch.clone_from(&source.state);
        Ok(anchor)
    }
}
impl<
    State: HistoryPayload,
    Input: HistoryPayload,
    const INPUTS: usize,
    const ANCHORS: usize,
    const BRANCHES: usize,
> InputStore<Input> for FixedHistory<State, Input, INPUTS, ANCHORS, BRANCHES>
{
    type Error = TimelineError;
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, Input>, TimelineError> {
        let source = match self.exact_input(record) {
            Ok(source) => source,
            Err(TimelineError::HistoryUnavailable) => self.source_input(record)?,
            Err(error) => return Err(error),
        };
        Ok(BoundInput {
            record,
            value: &source.payload,
        })
    }
}
impl<
    State: HistoryPayload + Clone,
    Input: HistoryPayload,
    const INPUTS: usize,
    const ANCHORS: usize,
    const BRANCHES: usize,
> SnapshotCapture<State> for FixedHistory<State, Input, INPUTS, ANCHORS, BRANCHES>
{
    type Error = TimelineError;
    fn capture(&mut self, anchor: StateAnchor, state: &State) -> Result<(), TimelineError> {
        if self.check_anchor_admission(anchor, state, true)?.is_none() {
            return Ok(());
        }
        self.insert_snapshot(anchor, state.clone())
    }
}

impl<
    State: HistoryPayload + Clone,
    Input: HistoryPayload,
    const INPUTS: usize,
    const ANCHORS: usize,
    const BRANCHES: usize,
> LiveSnapshotStore<State> for FixedHistory<State, Input, INPUTS, ANCHORS, BRANCHES>
{
    fn restore_current(
        &self,
        expected: TimelineHead,
        anchor: StateAnchor,
        scratch: &mut State,
    ) -> Result<StateAnchor, TimelineError> {
        self.check_scope(expected.scope())?;
        if self.snapshot_at(expected.branch(), expected.step())? != anchor {
            return Err(TimelineError::InputMismatch);
        }
        self.restore(anchor, scratch)
    }
}
