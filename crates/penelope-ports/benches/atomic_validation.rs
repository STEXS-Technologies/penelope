//! Public validator CPU latency, with the ordinary system allocator.
#![allow(clippy::unwrap_used)]
use std::{hint::black_box, time::Instant};
#[path = "../tests/support/atomic_validation_fixture.rs"]
mod fixture;
fn main() {
    fixture::commit(0).validate().unwrap();
    let long = std::env::var_os("PENELOPE_BENCH_LONG_IDS").is_some();
    let many = std::env::var_os("PENELOPE_BENCH_MANY_OUTCOMES").is_some();
    for (actions, iterations) in [(0, 100_000_u32), (1, 100_000), (16, 10_000), (128, 1_000)] {
        let iterations = if long { iterations / 10 } else { iterations };
        let iterations = if many {
            (iterations / 100).max(1)
        } else {
            iterations
        };
        let mut batch = fixture::commit_with_identifiers(actions, long);
        if std::env::var_os("PENELOPE_BENCH_VARY_PAYLOAD").is_some() {
            fixture::vary_payloads(&mut batch);
        }
        if many {
            let template = batch.outcomes.first().unwrap().clone();
            for sequence in 1..128_u64 {
                let mut outcome = template.clone();
                outcome.sequence = sequence;
                outcome.outcome_id = if long {
                    format!("out_{sequence:0124}")
                } else {
                    format!("out_{sequence:04}")
                }
                .parse()
                .unwrap();
                outcome.kind = penelope_domain::ProcessOutcomeKind::ActionPlanned;
                batch.outcomes.push(outcome);
            }
        }
        let checksum = serde_json::to_string(&penelope_domain::ContentDigest::sha256(
            &serde_json::to_vec(&batch).unwrap(),
        ))
        .unwrap();
        for _ in 0..100 {
            black_box(black_box(&batch).validate()).unwrap();
        }
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(black_box(&batch).validate()).unwrap();
        }
        let elapsed_ns = started.elapsed().as_nanos();
        println!(
            "actions={actions} iterations={iterations} elapsed_ns={elapsed_ns} checksum={checksum}"
        );
    }
}
