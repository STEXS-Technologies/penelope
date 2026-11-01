//! Handoff compaction must finish before any retired payload destructor runs.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
struct Payload {
    value: u64,
    panic_on: Rc<Cell<Option<u64>>>,
    dropped: Rc<RefCell<Vec<u64>>>,
}
impl Drop for Payload {
    fn drop(&mut self) {
        self.dropped.borrow_mut().push(self.value);
        if self.panic_on.get() == Some(self.value) {
            self.panic_on.set(None);
            panic!("injected acknowledged effect destructor panic");
        }
    }
}
struct Model {
    first: u64,
    count: u64,
    panic_on: Rc<Cell<Option<u64>>>,
    dropped: Rc<RefCell<Vec<u64>>>,
}
impl RewindableComputation for Model {
    type State = u64;
    type Input = u64;
    type Effect = Payload;
    type Error = TimelineError;
    fn step<S: EffectSink<Effect = Payload>>(
        &self,
        state: &mut u64,
        input: &u64,
        _context: StepContext,
        sink: &mut S,
    ) -> Result<(), TimelineError> {
        *state = state
            .checked_add(*input)
            .ok_or(TimelineError::CounterExhausted)?;
        for offset in 0..self.count {
            let value = self
                .first
                .checked_add(offset)
                .ok_or(TimelineError::CounterExhausted)?;
            let mut identity = [0; 32];
            identity[..8].copy_from_slice(&value.to_be_bytes());
            sink.stage(
                TimelineEffectId::from_digest(Digest(identity)),
                value.digest(),
                8,
                Payload {
                    value,
                    panic_on: Rc::clone(&self.panic_on),
                    dropped: Rc::clone(&self.dropped),
                },
            )?;
        }
        Ok(())
    }
}
struct Ack(Vec<u64>);
impl EffectHandoff<Payload> for Ack {
    type Error = TimelineError;
    fn submit(
        &mut self,
        effect: EligibleEffect<'_, Payload>,
    ) -> Result<EffectAdmissionReceipt, TimelineError> {
        self.0.push(effect.payload().value);
        Ok(EffectAdmissionReceipt {
            descriptor: effect.descriptor(),
            durable_commit: Digest([9; 32]),
        })
    }
}
fn scenario<const C: usize>(count: u64, budget: usize) {
    let scope = TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    };
    let head = TimelineHead::new(
        scope,
        TimelineBranchId::ZERO,
        TimelineStep::ZERO,
        TimelineRevision::ZERO,
        None,
    )
    .unwrap();
    let mut live = LiveTimeline::<u64, Payload, 1, C>::new(
        head,
        0,
        EffectLimits {
            max_intents: C,
            max_payload_bytes: u64::try_from(C).unwrap() * 8,
        },
    );
    let panic_on = Rc::new(Cell::new(None));
    let dropped = Rc::new(RefCell::new(Vec::new()));
    let mut model = Model {
        first: 1,
        count,
        panic_on: Rc::clone(&panic_on),
        dropped: Rc::clone(&dropped),
    };
    let valid = |_state: &u64, _head| Ok(());
    live.apply_input(
        &model,
        TimelineInputId::from_digest(Digest([5; 32])),
        &1,
        &valid,
    )
    .unwrap();
    live.finalize_effects(live.read().0, TimelineStep::new(1))
        .unwrap();
    let before = live.read().0;
    let mut original: Vec<_> = live
        .eligible_effects()
        .map(|effect| {
            (
                effect.descriptor().identity.digest().0,
                effect.payload().value,
            )
        })
        .collect();
    original.sort_unstable();
    let retired = original.first().unwrap().1;
    panic_on.set(Some(retired));
    let mut adapter = Ack(Vec::new());
    assert!(catch_unwind(AssertUnwindSafe(|| live.handoff(&mut adapter, budget))).is_err());
    assert_eq!(adapter.0, vec![retired]);
    assert_eq!(live.read(), (before, &1));
    let remaining: BTreeSet<_> = live
        .tentative_effects()
        .map(|effect| effect.payload.value)
        .collect();
    assert_eq!(
        remaining,
        original.iter().skip(1).map(|(_, value)| *value).collect()
    );
    assert_eq!(
        live.tentative_effects().count(),
        usize::try_from(count - 1).unwrap()
    );
    // Append after removing a middle slot, then drain only the finalized prefix.
    model.first = 100;
    model.count = 1;
    live.apply_input(
        &model,
        TimelineInputId::from_digest(Digest([6; 32])),
        &1,
        &valid,
    )
    .unwrap();
    assert_eq!(
        live.handoff(&mut adapter, usize::MAX).unwrap(),
        usize::try_from(count - 1).unwrap()
    );
    assert_eq!(
        adapter.0,
        original.iter().map(|(_, value)| *value).collect::<Vec<_>>()
    );
    assert_eq!(live.tentative_effects().count(), 1);
    live.finalize_effects(live.read().0, TimelineStep::new(2))
        .unwrap();
    assert_eq!(live.handoff(&mut adapter, 8).unwrap(), 1);
    assert_eq!(live.tentative_effects().count(), 0);
    let mut all_dropped = dropped.borrow().clone();
    all_dropped.sort_unstable();
    assert_eq!(
        all_dropped,
        (1..=count).chain(core::iter::once(100)).collect::<Vec<_>>()
    );
}

#[test]
fn acknowledged_effect_drop_panic_keeps_remaining_slots_usable_and_exact() {
    scenario::<8>(3, 1); // Middle-slot scan/compaction.
    scenario::<32>(16, 32); // Ordered bulk tail-pop.
}
