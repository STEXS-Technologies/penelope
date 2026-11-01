//! Borrowed immutable replay plans. This module neither executes nor publishes state.
use super::*;

/// Validated range pinned to one complete expected head and immutable input slice.
///
/// Only metadata is validated. Trust, snapshot completeness, actual execution and
/// atomic state/head publication require cooperating consumer adapters.
#[derive(Debug)]
pub struct ReplayPlan<'inputs> {
    expected: TimelineHead,
    anchor: StateAnchor,
    replacement: Option<InputRecord>,
    inputs: &'inputs [InputRecord],
}

impl<'inputs> ReplayPlan<'inputs> {
    /// Checks a whole-scope correction against a pinned contiguous input range.
    ///
    /// `inputs` contains the original records strictly after the restore anchor,
    /// including the original record replaced by an input correction. An empty
    /// range is valid for a state correction at the current head. Correction
    /// authority and anchor lineage must already be admitted by the consumer.
    /// # Errors
    /// Refuses stale heads, scope/branch errors, gaps, future corrections,
    /// insufficient capacity, counter exhaustion and effects-finality conflicts.
    pub fn prepare(
        expected: TimelineHead,
        expected_revision: TimelineRevision,
        correction: Correction,
        inputs: &'inputs [InputRecord],
        limits: ReplayLimits,
    ) -> Result<Self, TimelineError> {
        if expected.revision != expected_revision {
            return Err(TimelineError::Conflict);
        }
        // Promotion needs a fresh epoch; fail before doing any replay work.
        expected
            .revision
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        let (anchor, replacement) = match correction {
            Correction::State(anchor) => (anchor, None),
            Correction::Input {
                anchor,
                replacement,
            } => (anchor, Some(replacement)),
        };
        if anchor.scope != expected.scope {
            return Err(TimelineError::ScopeMismatch);
        }
        if anchor.branch != expected.branch {
            return Err(TimelineError::Conflict);
        }
        if anchor.step > expected.step {
            return Err(TimelineError::FutureCorrection);
        }
        if let Some(record) = replacement {
            if record.scope != expected.scope {
                return Err(TimelineError::ScopeMismatch);
            }
            if record.branch != expected.branch {
                return Err(TimelineError::Conflict);
            }
            let first = anchor
                .step
                .checked_next()
                .ok_or(TimelineError::CounterExhausted)?;
            if record.step != first {
                return Err(TimelineError::HistoryUnavailable);
            }
            if record.step > expected.step {
                return Err(TimelineError::FutureCorrection);
            }
            if expected
                .effect_finalized
                .is_some_and(|frontier| record.step <= frontier)
            {
                return Err(TimelineError::ReconciliationRequired);
            }
        } else if expected
            .effect_finalized
            .is_some_and(|frontier| anchor.step <= frontier)
        {
            // State replacement at the frontier can contradict an issued effect.
            return Err(TimelineError::ReconciliationRequired);
        }
        let count = expected
            .step
            .get()
            .checked_sub(anchor.step.get())
            .ok_or(TimelineError::HistoryUnavailable)?;
        let supplied = u64::try_from(inputs.len()).map_err(|_error| TimelineError::Capacity)?;
        if supplied > limits.max_retained_inputs || count > limits.max_replay_steps {
            return Err(TimelineError::Capacity);
        }
        if supplied != count {
            return Err(TimelineError::HistoryUnavailable);
        }
        let mut cursor = anchor.step;
        for record in inputs {
            cursor = cursor
                .checked_next()
                .ok_or(TimelineError::CounterExhausted)?;
            if record.scope != expected.scope {
                return Err(TimelineError::ScopeMismatch);
            }
            if record.branch != expected.branch {
                return Err(TimelineError::Conflict);
            }
            if record.step != cursor {
                return Err(TimelineError::HistoryUnavailable);
            }
        }
        if let Some(record) = replacement {
            let original = inputs.first().ok_or(TimelineError::HistoryUnavailable)?;
            if record.identity != original.identity || record.revision <= original.revision {
                return Err(TimelineError::InputMismatch);
            }
        }
        Ok(Self {
            expected,
            anchor,
            replacement,
            inputs,
        })
    }
    /// Corrected input descriptor, if this plan replaces an input rather than state.
    #[must_use]
    pub const fn replacement_input(&self) -> Option<InputRecord> {
        self.replacement
    }
    /// Logical step whose state or input is corrected.
    #[must_use]
    pub const fn corrected_step(&self) -> TimelineStep {
        match self.replacement {
            Some(record) => record.step,
            None => self.anchor.step,
        }
    }
    /// Identity of the corrected state or input payload (not proof of authority).
    #[must_use]
    pub const fn corrected_digest(&self) -> crate::task::Digest {
        match self.replacement {
            Some(record) => record.digest,
            None => self.anchor.digest,
        }
    }
    /// Expected atomic-publication head, including its finality frontier.
    #[must_use]
    pub const fn expected_head(&self) -> TimelineHead {
        self.expected
    }
    /// Complete state to restore into consumer-owned scratch memory.
    #[must_use]
    pub const fn anchor(&self) -> StateAnchor {
        self.anchor
    }
    /// Number of steps required after restoring the anchor.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.inputs.len()
    }
    /// Whether correction needs no subsequent inputs.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }
    /// Exact record at a replay offset, substituting a corrected first input.
    #[must_use]
    pub fn input(&self, offset: usize) -> Option<InputRecord> {
        match (offset, self.replacement) {
            (0, Some(record)) => Some(record),
            _ => self.inputs.get(offset).copied(),
        }
    }
    /// Rejects a candidate if any live-head field changed during replay.
    /// # Errors
    /// Returns `Conflict` when the candidate no longer targets the current head.
    pub fn check_head(&self, current: TimelineHead) -> Result<(), TimelineError> {
        if self.expected != current {
            return Err(TimelineError::Conflict);
        }
        Ok(())
    }
}
