#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]

use penelope_domain::{
    CanonicalWireBytes, CausationId, ContentDigest, LogicalTime, OutcomeActor, OutcomeId,
    ProcessOutcome, ProcessOutcomeFact, ProcessOutcomeKind, ProcessScope,
};
use penelope_ports::OutcomeLog;
use std::time::Instant;

fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
    T::try_from(value)
        .ok()
        .expect("benchmark identifier is valid")
}

fn scope() -> ProcessScope {
    ProcessScope::new(
        id("tnt_bench"),
        id("prc_bench"),
        id("def_bench"),
        id("dfv_one"),
        ContentDigest([3; 32]),
    )
}

fn outcome(sequence: u64) -> ProcessOutcome {
    ProcessOutcome::new(
        scope(),
        sequence,
        ProcessOutcomeFact::new(
            OutcomeId::try_from(format!("out_bench_{sequence}"))
                .expect("benchmark outcome identifier is valid"),
            CausationId::Action(id("act_bench")),
            OutcomeActor::System,
            LogicalTime(sequence),
            ProcessOutcomeKind::ActionPlanned,
            ContentDigest([4; 32]),
        ),
    )
}

fn main() {
    let iterations = 100_000_u64;
    let value = outcome(0);
    let encoding_start = Instant::now();
    let mut encoded_bytes = 0_usize;
    for _ in 0..iterations {
        encoded_bytes = encoded_bytes.saturating_add(
            value
                .canonical_wire_bytes()
                .expect("domain DTO serialization is supported")
                .len(),
        );
    }
    let encoding_elapsed = encoding_start.elapsed();
    println!(
        "canonical DTO encoding: {iterations} operations in {encoding_elapsed:?} ({} ns/op, {encoded_bytes} bytes)",
        encoding_elapsed.as_nanos() / u128::from(iterations)
    );

    let mut log = OutcomeLog::new(scope());
    let append_start = Instant::now();
    for sequence in 0..iterations.min(4096) {
        let item = ProcessOutcome::new(
            scope(),
            sequence,
            ProcessOutcomeFact::new(
                OutcomeId::try_from(format!("out_bench_{sequence}"))
                    .expect("benchmark outcome identifier is valid"),
                CausationId::Action(id("act_bench")),
                OutcomeActor::System,
                LogicalTime(sequence),
                ProcessOutcomeKind::ActionPlanned,
                ContentDigest([4; 32]),
            ),
        );
        assert!(log.append(&[item]).is_ok());
    }
    let append_elapsed = append_start.elapsed();
    println!(
        "bounded outcome-log append: {} operations in {append_elapsed:?} ({} ns/op)",
        log.len(),
        append_elapsed.as_nanos() / u128::from(log.len().max(1) as u64)
    );
}
