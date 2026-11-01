//! Retained-history paths, including inherited aliases and slots left by eviction.
use super::*;

pub fn profiles(report: &mut impl Reporter) {
    let fixture = Fixture::<1>::new();
    for n in [1, 8, 32, 128, 512] {
        for layout in ["root", "inherited", "evicted"] {
            let end = if layout == "evicted" { WINDOW } else { n };
            let after = end - n;
            let baseline = anchor(after, fixture.expected(after));
            let mut history = fixture.history(end);
            let mut current = head(end);
            if layout == "inherited" {
                let plan = fixture.plan(end);
                let mut live = fixture.live(end);
                let reservation = live.reserve(&plan, observation(fixture.anchor)).unwrap();
                history.fork(&plan, reservation).unwrap();
                current = reservation.proposed_head().unwrap();
            } else if after != 0 {
                history
                    .insert_snapshot(baseline, fixture.expected(after))
                    .unwrap();
                history.expire_before(baseline.step).unwrap();
            }
            let mut snapshot = baseline;
            snapshot.branch = current.branch();
            let mut storage = fixture.records;
            let records = history
                .collect_inputs(
                    current.branch(),
                    baseline.step,
                    current.step(),
                    &mut storage,
                )
                .unwrap();
            assert_eq!(records.len(), n);
            for (record, expected) in records.iter().zip(&fixture.values[after..end]) {
                assert_eq!(*history.read(*record).unwrap().value, *expected);
            }
            let expected = fixture.expected(end);
            let full = |live: &mut Live<1>| {
                let mut scratch = fixture.records;
                let selected_records = history
                    .collect_inputs(
                        current.branch(),
                        baseline.step,
                        current.step(),
                        &mut scratch,
                    )
                    .unwrap();
                let plan = ReplayPlan::prepare(
                    current,
                    current.revision(),
                    Correction::State(snapshot),
                    selected_records,
                    LIMITS,
                )
                .unwrap();
                let mut observed = observation(snapshot);
                observed.sequence = AuthoritySequence::new(2);
                let reservation = live.reserve(&plan, observed).unwrap();
                let mut session = history
                    .begin_replay::<(), 0>(plan, reservation, State([0]), FX)
                    .unwrap();
                session
                    .advance(current, &Computation::<1>, n as u64)
                    .unwrap();
                assert_eq!(*session.scratch(), expected);
                live.promote(session.validate(current, &Validator(expected)).unwrap())
                    .unwrap()
            };
            let mut checked = Live::new(current, expected, FX);
            full(&mut checked);
            assert_eq!(*checked.read().1, expected);
            report.sample(
                &format!("history_read_all_{layout}"),
                1,
                n,
                || (),
                |()| {
                    records.iter().fold(0_u64, |sum, record| {
                        sum.wrapping_add(*history.read(black_box(*record)).unwrap().value)
                    })
                },
            );
            report.sample(
                &format!("history_collect_{layout}"),
                1,
                n,
                || fixture.records,
                |scratch| {
                    history
                        .collect_inputs(current.branch(), baseline.step, current.step(), scratch)
                        .unwrap()
                        .len()
                },
            );
            report.sample(
                &format!("history_full_correction_{layout}"),
                1,
                n,
                || Live::new(current, expected, FX),
                full,
            );
        }
    }
}
