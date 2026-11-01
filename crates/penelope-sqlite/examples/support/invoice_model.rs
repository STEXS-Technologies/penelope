//! Application-owned invoice model. No simulation types, storage or ambient I/O.
use super::ExampleError;
use penelope_core::task::Digest;
use penelope_timeline::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Account {
    pub(super) total: u64,
    pub(super) units: u64,
}
#[derive(Clone, Copy)]
struct Purchase {
    units: u64,
    price: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Invoice {
    pub(super) total: u64,
    pub(super) units: u64,
}
impl Invoice {
    pub(super) fn bytes(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        let (total, units) = bytes.split_at_mut(8);
        total.copy_from_slice(&self.total.to_le_bytes());
        units.copy_from_slice(&self.units.to_le_bytes());
        bytes
    }
    pub(super) fn from_bytes(bytes: [u8; 16]) -> Result<Self, TimelineError> {
        let (total, units) = bytes.split_at(8);
        Ok(Self {
            total: u64::from_le_bytes(
                total
                    .try_into()
                    .map_err(|_error| TimelineError::InputMismatch)?,
            ),
            units: u64::from_le_bytes(
                units
                    .try_into()
                    .map_err(|_error| TimelineError::InputMismatch)?,
            ),
        })
    }
}
impl HistoryPayload for Invoice {
    fn digest(&self) -> Digest {
        Digest::of(&self.bytes())
    }
    fn byte_len(&self) -> u64 {
        16
    }
}
impl HistoryPayload for Account {
    fn digest(&self) -> Digest {
        Invoice {
            total: self.total,
            units: self.units,
        }
        .digest()
    }
    fn byte_len(&self) -> u64 {
        16
    }
}
impl HistoryPayload for Purchase {
    fn digest(&self) -> Digest {
        Invoice {
            total: self.price,
            units: self.units,
        }
        .digest()
    }
    fn byte_len(&self) -> u64 {
        16
    }
}
pub(super) const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([11; 32])),
        timeline: TimelineId::from_digest(Digest([12; 32])),
        model: TimelineModelId::from_digest(Digest([13; 32])),
        schema: TimelineSchemaId::from_digest(Digest([14; 32])),
    }
}
struct Pricing;
impl RewindableComputation for Pricing {
    type State = Account;
    type Input = Purchase;
    type Effect = Invoice;
    type Error = TimelineError;
    fn step<S: EffectSink<Effect = Invoice>>(
        &self,
        state: &mut Account,
        input: &Purchase,
        context: StepContext,
        effects: &mut S,
    ) -> Result<(), TimelineError> {
        let charge = input
            .units
            .checked_mul(input.price)
            .ok_or(TimelineError::ValidationFailed)?;
        state.total = state
            .total
            .checked_add(charge)
            .ok_or(TimelineError::ValidationFailed)?;
        state.units = state
            .units
            .checked_add(input.units)
            .ok_or(TimelineError::ValidationFailed)?;
        if context.step == TimelineStep::new(3) {
            let invoice = Invoice {
                total: state.total,
                units: state.units,
            };
            effects.stage(
                TimelineEffectId::from_digest(Digest::of(b"invoice-summary")),
                invoice.digest(),
                invoice.byte_len(),
                invoice,
            )?;
        }
        Ok(())
    }
}
fn anchor(branch: TimelineBranchId, step: TimelineStep, state: Account) -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch,
        step,
        snapshot: TimelineSnapshotId::from_digest(state.digest()),
        digest: state.digest(),
    }
}
fn validator(expected: Account) -> DigestValidator<fn(&Account) -> Digest> {
    DigestValidator {
        expected: expected.digest(),
        digest: Account::digest,
    }
}
pub(super) type Live = LiveTimeline<Account, Invoice, 2, 4>;
// This acceptance example deliberately asserts the observable contract.
#[allow(clippy::panic_in_result_fn)]
pub(super) fn corrected_invoice() -> Result<Live, ExampleError> {
    let initial = Account::default();
    let genesis = TimelineHead::new(
        scope(),
        TimelineBranchId::ZERO,
        TimelineStep::ZERO,
        TimelineRevision::ZERO,
        None,
    )?;
    let fx = EffectLimits {
        max_intents: 4,
        max_payload_bytes: 64,
    };
    let mut history = FixedHistory::<Account, Purchase, 8, 8, 4>::new(
        anchor(genesis.branch(), genesis.step(), initial),
        initial,
        HistoryLimits {
            max_input_bytes: 16,
            max_snapshot_bytes: 16,
            max_total_bytes: 1024,
        },
    )?;
    let mut live = Live::new(genesis, initial, fx);
    let purchases = [
        Purchase {
            units: 2,
            price: 10,
        },
        Purchase { units: 1, price: 5 },
        Purchase { units: 3, price: 7 },
    ];
    let mut expected = initial;
    for purchase in purchases {
        let current = live.read().0;
        let step = current
            .step()
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        let input = InputRecord {
            scope: scope(),
            branch: current.branch(),
            step,
            identity: TimelineInputId::from_digest(Digest::of(&step.get().to_le_bytes())),
            revision: InputRevision::ZERO,
            digest: purchase.digest(),
        };
        history.append_input(input, purchase)?;
        // Independent arithmetic expectation for the uninterrupted prediction.
        expected.total = expected
            .total
            .checked_add(
                purchase
                    .units
                    .checked_mul(purchase.price)
                    .ok_or(TimelineError::ValidationFailed)?,
            )
            .ok_or(TimelineError::ValidationFailed)?;
        expected.units = expected
            .units
            .checked_add(purchase.units)
            .ok_or(TimelineError::ValidationFailed)?;
        let ready = prepare_live_step::<_, _, _, _, 4>(
            LiveStepRequest {
                expected: current,
                anchor: history.snapshot_at(current.branch(), current.step())?,
                input,
                scratch: initial,
                limits: fx,
            },
            &Pricing,
            &history,
            &history,
            &validator(expected),
        )?;
        let next = live.append(ready)?;
        history.capture(
            anchor(next.branch(), next.step(), *live.read().1),
            live.read().1,
        )?;
    }
    assert_eq!(live.read().1.total, 46);
    assert_eq!(live.tentative_effects().count(), 1);
    assert_eq!(live.eligible_effects().count(), 0);
    // Application-authenticated delayed pricing result: the first purchase cost 18.
    let corrected_state = Account {
        total: 18,
        units: 2,
    };
    let old = live.read().0;
    let corrected = anchor(old.branch(), TimelineStep::new(1), corrected_state);
    history.import_anchor(corrected, corrected_state)?;
    let observed = live.observe(
        old,
        AuthorityObservation {
            scope: scope(),
            authority: TimelineAuthorityId::from_digest(Digest([15; 32])),
            epoch: AuthorityEpoch::ZERO,
            sequence: AuthoritySequence::new(1),
            step: corrected.step,
            digest: corrected.digest,
        },
    )?;
    assert_eq!(observed.effect_finalized(), None);
    let filler = history.input_at(old.branch(), TimelineStep::new(1))?;
    let mut scratch = [filler; 8];
    let inputs = history.collect_inputs(
        observed.branch(),
        corrected.step,
        observed.step(),
        &mut scratch,
    )?;
    let plan = ReplayPlan::prepare(
        observed,
        observed.revision(),
        Correction::State(corrected),
        inputs,
        ReplayLimits {
            max_retained_inputs: 8,
            max_replay_steps: 8,
        },
    )?;
    let reservation = live.reserve(
        &plan,
        observed.authority().ok_or(TimelineError::StaleAuthority)?,
    )?;
    history.fork(&plan, reservation)?;
    let mut replay = history.begin_replay::<Invoice, 4>(plan, reservation, initial, fx)?;
    replay.advance(observed, &Pricing, 1)?;
    assert_eq!(live.read().1.total, 46);
    replay.advance(observed, &Pricing, 1)?;
    let expected_corrected = Account {
        total: 44,
        units: 6,
    }; // 18 + 1*5 + 3*7.
    let candidate = replay.validate(observed, &validator(expected_corrected))?;
    live.promote(candidate)?;
    assert_eq!(*live.read().1, expected_corrected);
    assert_eq!(live.tentative_effects().count(), 1); // The speculative 46 invoice was replaced.
    assert_eq!(live.eligible_effects().count(), 0); // Observation/promotion did not finalize it.
    Ok(live)
}
