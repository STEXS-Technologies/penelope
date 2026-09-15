#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]

use penelope_domain::{
    CanonicalWireBytesV1, CausationIdV1, ContentDigest, LogicalTimeV1, OutcomeActorV1, OutcomeId,
    ProcessOutcomeDtoV1, ProcessOutcomeFactV1, ProcessOutcomeKindV1, ProcessScopeV1,
};
use penelope_ports::OutcomeLogV1;
use std::time::Instant;

fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
    T::try_from(value)
        .ok()
        .expect("benchmark identifier is valid")
}

fn scope() -> ProcessScopeV1 {
    ProcessScopeV1::new(
        id("tnt_bench"),
        id("prc_bench"),
        id("def_bench"),
        id("dfv_one"),
        ContentDigest([3; 32]),
    )
}

fn outcome(sequence: u64) -> ProcessOutcomeDtoV1 {
    ProcessOutcomeDtoV1::new(
        scope(),
        sequence,
        ProcessOutcomeFactV1::new(
            OutcomeId::try_from(format!("out_bench_{sequence}"))
                .expect("benchmark outcome identifier is valid"),
            CausationIdV1::Action(id("act_bench")),
            OutcomeActorV1::System,
            LogicalTimeV1(sequence),
            ProcessOutcomeKindV1::ActionPlanned,
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

    let mut log = OutcomeLogV1::new(scope());
    let append_start = Instant::now();
    for sequence in 0..iterations.min(4096) {
        let item = ProcessOutcomeDtoV1::new(
            scope(),
            sequence,
            ProcessOutcomeFactV1::new(
                OutcomeId::try_from(format!("out_bench_{sequence}"))
                    .expect("benchmark outcome identifier is valid"),
                CausationIdV1::Action(id("act_bench")),
                OutcomeActorV1::System,
                LogicalTimeV1(sequence),
                ProcessOutcomeKindV1::ActionPlanned,
                ContentDigest([4; 32]),
            ),
        );
        assert!(log.append(&[item]).is_ok());
    }
    let append_elapsed = append_start.elapsed();
    println!(
        "bounded outcome-log append: {} operations in {append_elapsed:?} ({} ns/op)",
        log.outcomes.len(),
        append_elapsed.as_nanos() / u128::from(log.outcomes.len().max(1) as u64)
    );
}
