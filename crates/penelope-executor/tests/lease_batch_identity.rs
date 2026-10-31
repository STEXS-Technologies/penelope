//! Earliest conflict and validation precedence remain stable across batch sizes.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
#[path = "support/lease_batch_fixture.rs"]
mod fixture;
#[path = "../src/lease_batch_index.rs"]
mod implementation;
use penelope_domain::LogicalTime;
use penelope_executor::services::{TimerDrainStage, claim_outbox, drain_timers};
use penelope_ports::PortError;
fn old(leases: &[(String, u64)]) -> Option<usize> {
    leases.iter().enumerate().find_map(|(index, lease)| {
        leases
            .iter()
            .skip(index + 1)
            .any(|other| other.0 == lease.0 || other.1 == lease.1)
            .then_some(index)
    })
}
#[test]
fn earliest_conflict_matches_pair_scan_for_every_duplicate_pair() {
    let mut cases = 0;
    for width in [9, 128] {
        for count in [0, 1, 2, 8, 16, 17, 32, 128, 129] {
            let base: Vec<_> = (0..count)
                .map(|i| {
                    (
                        format!("act_{}{:04}", "x".repeat(width - 8), i),
                        u64::try_from(i + 1).unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                implementation::first_duplicate_index(&base, |a| (a.0.as_str(), a.1)),
                old(&base)
            );
            cases += 1;
            if count == 129 {
                continue;
            }
            for first in 0..count {
                for last in first + 1..count {
                    for token in [false, true] {
                        let mut leases = base.clone();
                        if token {
                            leases[last].1 = leases[first].1;
                        } else {
                            leases[last].0 = leases[first].0.clone();
                        }
                        assert_eq!(
                            implementation::first_duplicate_index(&leases, |a| (a.0.as_str(), a.1)),
                            old(&leases)
                        );
                        cases += 1;
                    }
                }
            }
            if count >= 4 {
                let mut leases = base;
                leases[count - 1].0 = leases[count - 2].0.clone();
                leases[count - 1].1 = leases[0].1;
                assert_eq!(
                    implementation::first_duplicate_index(&leases, |a| (a.0.as_str(), a.1)),
                    Some(0)
                );
                cases += 1;
            }
        }
    }
    println!("lease_batch_pair_cases={cases} first_conflict=true");
}
#[test]
fn public_batches_preserve_timeout_precedence_and_fail_before_delivery() {
    let mut cases = 0;
    for count in [16, 17, 32, 128] {
        for width in [9, 128] {
            for order in [0, 1, 2] {
                let baseline = fixture::Fixture::new(width, count, order).unwrap();
                for timers in [false, true] {
                    let ports = baseline.prepare(timers);
                    let output = baseline.invoke(&ports, timers).unwrap();
                    baseline.verify(&ports, &output).unwrap();
                }
                for first in [0, count / 2, count - 2] {
                    for bad in [0, first, count - 1] {
                        for token in [false, true] {
                            for timers in [false, true] {
                                let mut fixture = baseline.clone();
                                if timers {
                                    if token {
                                        fixture.timers[count - 1].token =
                                            fixture.timers[first].token;
                                    } else {
                                        fixture.timers[count - 1].timer.action.action_id =
                                            fixture.timers[first].timer.action.action_id.clone();
                                    }
                                    fixture.timers[bad].lease_expires_at = LogicalTime(0);
                                } else {
                                    if token {
                                        fixture.outbox[count - 1].token =
                                            fixture.outbox[first].token;
                                    } else {
                                        fixture.outbox[count - 1].record.action.action_id =
                                            fixture.outbox[first].record.action.action_id.clone();
                                    }
                                    fixture.outbox[bad].lease_expires_at = LogicalTime(0);
                                }
                                let ports = fixture.prepare(timers);
                                let expected = if bad <= first {
                                    PortError::TimedOut
                                } else {
                                    PortError::Invariant
                                };
                                let checked = if timers {
                                    implementation::validate_batch(
                                        &fixture.timers,
                                        |lease| {
                                            (lease.timer.action.action_id.as_str(), lease.token)
                                        },
                                        |lease| lease.validate_for_claim(&fixture.timer_request),
                                    )
                                } else {
                                    implementation::validate_batch(
                                        &fixture.outbox,
                                        |lease| {
                                            (lease.record.action.action_id.as_str(), lease.token)
                                        },
                                        |lease| {
                                            lease.validate_for_claim_at(
                                                &fixture.outbox_request,
                                                LogicalTime(100),
                                            )
                                        },
                                    )
                                };
                                assert_eq!(checked, Err(expected));
                                if timers {
                                    let failure = fixture::run(drain_timers(
                                        &ports,
                                        &ports,
                                        &ports,
                                        &fixture.timer_request,
                                        &ports,
                                    ))
                                    .unwrap_err();
                                    assert_eq!(failure.stage, TimerDrainStage::Validate);
                                    assert_eq!(failure.source, expected);
                                    assert_eq!(usize::from(failure.report.claimed), count);
                                    assert_eq!(failure.report.acknowledged, 0);
                                } else {
                                    assert_eq!(
                                        fixture::run(claim_outbox(
                                            &ports,
                                            &ports,
                                            &fixture.outbox_request,
                                            LogicalTime(100)
                                        ))
                                        .unwrap_err(),
                                        expected
                                    );
                                }
                                assert!(ports.payloads.lock().unwrap().is_empty());
                                assert!(ports.inputs.lock().unwrap().is_empty());
                                assert!(ports.acknowledgements.lock().unwrap().is_empty());
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 864);
    println!("lease_batch_public_fault_cases={cases} timeout_precedence=true no_delivery=true");
}
