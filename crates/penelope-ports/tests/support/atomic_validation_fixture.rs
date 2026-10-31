use penelope_domain::*;
use penelope_ports::*;
pub fn commit(actions: usize) -> AtomicProcessCommit {
    commit_with_identifiers(actions, false)
}
fn identifier<T: std::str::FromStr>(value: &str, long: bool) -> T {
    let text = if long {
        format!(
            "{value}{}",
            "x".repeat(128_usize.saturating_sub(value.len()))
        )
    } else {
        value.to_owned()
    };
    text.parse().ok().unwrap()
}
pub fn commit_with_identifiers(actions: usize, long: bool) -> AtomicProcessCommit {
    let scope = ProcessScope::new(
        identifier("tnt_bench", long),
        identifier("prc_bench", long),
        identifier("def_bench", long),
        identifier("dfv_one", long),
        ContentDigest([1; 32]),
    );
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        identifier("inp_start", long),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([2; 32]),
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            identifier("out_start", long),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::Started,
            ContentDigest([3; 32]),
        ),
    );
    AtomicProcessCommit {
        expected_sequence: 0,
        input: Some(input),
        canonical_source_event_id: None,
        outcomes: vec![outcome],
        actions: (0..actions)
            .map(|index| {
                ProcessAction::new(
                    scope.clone(),
                    identifier(&format!("act_{index:03}"), long),
                    identifier(&format!("stp_{index:03}"), long),
                    0,
                    ProcessActionKind::ExternalEffect,
                    ContentDigest([4; 32]),
                )
            })
            .collect(),
    }
}

pub fn vary_payloads(batch: &mut AtomicProcessCommit) {
    let Some(step) = batch.actions.first().map(|action| action.step_id.clone()) else {
        return;
    };
    for (index, action) in batch.actions.iter_mut().enumerate() {
        action.step_id = step.clone();
        action.payload_digest = ContentDigest([u8::try_from(index).unwrap(); 32]);
    }
}
