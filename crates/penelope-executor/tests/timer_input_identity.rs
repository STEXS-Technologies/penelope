//! Timer input and payload bytes remain stable across lease redelivery metadata.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
#[path = "support/timer_input_fixture.rs"]
mod fixture;
#[path = "../src/timer_input.rs"]
mod implementation;
#[path = "support/timer_input_legacy.rs"]
mod legacy;
use penelope_domain::*;
use penelope_ports::OutboxLeaseToken;
use std::num::NonZeroU64;
#[test]
fn timer_inputs_match_legacy_across_sizes_boundaries_and_redelivery() {
    let varied = (0_usize..10000).map(|case| {
        fixture::lease(
            5 + case % 124,
            5 + case / 3 % 124,
            5 + case / 7 % 124,
            u64::try_from(case).unwrap(),
            u32::try_from(case).unwrap(),
        )
        .unwrap()
    });
    let mut leases: Vec<_> = varied.collect();
    for (_profile, template) in fixture::profiles().unwrap() {
        for due in [0, 100, u64::MAX] {
            for attempt in [0, u32::MAX] {
                for payload in [0, 255] {
                    let mut lease = template.clone();
                    lease.timer.due_at = LogicalTime(due);
                    lease.timer.action.attempt = attempt;
                    lease.timer.action.payload_digest = ContentDigest([payload; 32]);
                    leases.push(lease);
                }
            }
        }
    }
    let mut cases = 0;
    for lease in leases {
        let encoded = implementation::timer_input(&lease).unwrap();
        assert_eq!(encoded, legacy::timer_input(&lease).unwrap());
        assert_eq!(encoded.0.kind, ProcessInputKind::TimerFired);
        assert_eq!(encoded.0.payload_digest, ContentDigest::sha256(&encoded.1));
        assert_eq!(encoded.0.input_id.as_str().len(), 74);
        assert!(encoded.0.input_id.as_str().starts_with("inp_timer_"));
        for variant in 0..3 {
            let mut changed = lease.clone();
            match variant {
                0 => changed.owner = "pri_other".parse().unwrap(),
                1 => changed.token = OutboxLeaseToken::new(NonZeroU64::new(u64::MAX).unwrap()),
                _ => changed.lease_expires_at = LogicalTime(0),
            }
            assert_eq!(implementation::timer_input(&changed).unwrap(), encoded);
        }
        cases += 1;
    }
    assert_eq!(cases, 10192);
    println!(
        "timer_input_legacy_cases={cases} metadata_substitutions=30576 exact_input=true exact_payload=true integer_boundaries=true"
    );
}
