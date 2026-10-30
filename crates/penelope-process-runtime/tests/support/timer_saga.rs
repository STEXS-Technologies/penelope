//! Timer inbox acceptance must eventually become an engine-validated commit.
use super::*;
use penelope_executor::engine::*;
use penelope_ports::{inbox::PendingInbox, payloads::ProcessPayloadStore};
use std::num::{NonZeroU32, NonZeroU64};

fn prepare_decision(
    stores: &Stores,
    definition: &LinearSagaDefinition,
    input: &LinearSagaInput,
    accepted: Option<ProcessInput>,
) -> (SagaDecision, AtomicProcessCommit) {
    let events = stores.replay_events::<LinearSagaEvent>();
    let prior = if events.is_empty() {
        None
    } else {
        Some(
            replay(
                definition,
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                &events,
            )
            .unwrap()
            .projection,
        )
    };
    let decision = decide(
        definition,
        prior.as_ref(),
        stores.scope.tenant_id.clone(),
        stores.scope.process_id.clone(),
        input,
    )
    .unwrap();
    let event = input.to_event(definition);
    let digest = stores.persist(&event);
    let outcomes = run(stores.store.read_outcomes(
        &OutcomeReplayRequest::new(stores.scope.clone(), 0, NonZeroU16::new(32).unwrap()).unwrap(),
    ))
    .unwrap();
    assert!(outcomes.next_sequence.is_none());
    let sequence = u64::try_from(outcomes.outcomes.len()).unwrap();
    let input_id = event.input_id().unwrap();
    let facts = stores.facts(
        input_id,
        event
            .observed_outcome_kinds()
            .iter()
            .chain(decision.planned_outcome_kinds())
            .copied(),
        digest,
        sequence,
    );
    let required = decision
        .build_required_outcomes(&event, &stores.scope, sequence, &facts)
        .unwrap();
    let accepted = accepted.unwrap_or_else(|| {
        ProcessInput::new(
            stores.scope.tenant_id.clone(),
            stores.scope.process_id.clone(),
            input_id.clone(),
            ProcessInputKind::CanonicalEvent,
            digest,
        )
    });
    let commit = AtomicProcessCommit::new(
        sequence,
        Some(accepted),
        required,
        decision.next_action.iter().cloned().collect(),
    )
    .unwrap();
    (decision, commit)
}

fn commit_decision(
    stores: &Stores,
    definition: &LinearSagaDefinition,
    input: &LinearSagaInput,
    accepted: Option<ProcessInput>,
) -> (SagaDecision, AtomicProcessCommit) {
    let (decision, commit) = prepare_decision(stores, definition, input, accepted);
    run(stores.store.commit(&commit))
        .unwrap()
        .validate_for(&commit)
        .unwrap();
    (decision, commit)
}

fn acknowledge_dispatch(stores: &Stores, expected: &ProcessAction) {
    let request = OutboxClaimRequest::new(
        stores.scope.clone(),
        id("pri_timer_host"),
        NonZeroU16::new(1).unwrap(),
    )
    .unwrap();
    let leases = run(claim_outbox(
        &Policy {
            allow: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        },
        &stores.store,
        &request,
        LogicalTime(1),
    ))
    .unwrap();
    assert_eq!(leases.len(), 1);
    let lease = leases.first().unwrap();
    assert_eq!(&lease.record.action, expected);
    run(OutboxStore::acknowledge(
        &stores.store,
        lease,
        LogicalTime(1),
    ))
    .unwrap();
}

#[test]
fn discovered_timer_commits_one_retry_task_and_replays_terminal_saga() {
    for separate in [false, true] {
        let mut stores = Stores::new(separate, scope());
        let mut step = super::super::step();
        step.retry_policy = RetryPolicy::new(NonZeroU32::new(2).unwrap()).with_backoff(
            RetryBackoff::new(NonZeroU64::new(10).unwrap(), NonZeroU64::new(10).unwrap()).unwrap(),
        );
        let definition = LinearSagaDefinition::new(
            stores.scope.definition_id.clone(),
            stores.scope.definition_version.clone(),
            stores.scope.definition_digest,
            vec![step],
        );
        let (started, _) = commit_decision(
            &stores,
            &definition,
            &LinearSagaInput::Start {
                input_id: id("inp_start"),
                action_id: id("act_first"),
            },
            None,
        );
        let original = started.next_action.unwrap();
        // This fixture supplies an independently observed retryable failure;
        // dispatch acceptance is acknowledged separately from effect meaning.
        acknowledge_dispatch(&stores, &original);
        let (scheduled, _) = commit_decision(
            &stores,
            &definition,
            &LinearSagaInput::RetryTimerScheduled {
                input_id: id("inp_failure"),
                observation: ActionResultObservation::retryable_failure(original.action_id),
                next_action_id: Some(id("act_timer")),
                observed_at: LogicalTime(0),
                jitter_seed: RetryJitterSeed::from_digest(ContentDigest([1; 32])),
                retry_deadline: None,
            },
            None,
        );
        let timer = scheduled.retry_timer_schedule().unwrap();
        assert_eq!(timer.due_at, LogicalTime(10));
        run(stores.store.schedule(&timer)).unwrap();
        acknowledge_dispatch(&stores, &timer.action);
        let request = TimerClaimRequest::new(
            stores.scope.clone(),
            id("pri_timers"),
            LogicalTime(9999),
            NonZeroU16::new(2).unwrap(),
        )
        .unwrap();
        let policy = Policy {
            allow: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        };
        let delivered = run(drain_timers(
            &policy,
            &stores.store,
            &stores.store,
            &request,
            &Clock(AtomicU64::new(10)),
        ))
        .unwrap();
        assert_eq!(delivered.acknowledged, 1);
        stores.reopen();
        let pending = run(stores.store.pending_inputs(
            &stores.scope.tenant_id,
            &stores.scope.process_id,
            NonZeroU16::new(2).unwrap(),
        ))
        .unwrap();
        assert_eq!(pending.len(), 1);
        let accepted = pending.first().unwrap();
        assert_eq!(accepted.kind, ProcessInputKind::TimerFired);
        // Bind the discovered tenant/process input to the exact replayed timer
        // and complete definition scope before constructing the engine input.
        let timer_bytes = timer.canonical_wire_bytes().unwrap();
        assert_eq!(accepted.payload_digest, ContentDigest::sha256(&timer_bytes));
        assert_eq!(
            run(stores
                .store
                .read_payload(&stores.scope, accepted.payload_digest))
            .unwrap(),
            timer_bytes
        );
        let before_fire = replay(
            &definition,
            &stores.scope.tenant_id,
            &stores.scope.process_id,
            &stores.replay_events(),
        )
        .unwrap();
        assert_eq!(before_fire.retry_timer_schedule().unwrap(), timer);
        let fire = LinearSagaInput::RetryTimerFired {
            input_id: accepted.input_id.clone(),
            timer_action_id: timer.action.action_id,
            fired_at: LogicalTime(10),
            next_action_id: id("act_retry"),
        };
        let (retried, retry_commit) =
            commit_decision(&stores, &definition, &fire, Some(accepted.clone()));
        assert_eq!(retried.next_action.as_ref().unwrap().attempt, 1);
        assert!(
            run(stores.store.pending_inputs(
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                NonZeroU16::new(2).unwrap()
            ))
            .unwrap()
            .is_empty()
        );
        // Lose the process receipt and retry the immutable timer commit. The
        // only accepted retry action remains act_retry, with its original key.
        assert!(
            run(stores.store.commit(&retry_commit))
                .unwrap()
                .duplicate_input
        );
        let relay = stores.execute(&retry_commit);
        let (completed, result_commit) = prepare_decision(
            &stores,
            &definition,
            &relay.linear_input(None),
            Some(relay.input().clone()),
        );
        run(penelope_executor::services::relay_task_result(
            &super::super::RelayPolicy {
                scope: stores.scope.clone(),
                allow: true,
            },
            &id("pri_relay"),
            &relay,
            &stores.store,
            &result_commit,
        ))
        .unwrap();
        assert_eq!(completed.projection.status, SagaStatus::Completed);
        stores.reopen();
        let replayed = replay(
            &definition,
            &stores.scope.tenant_id,
            &stores.scope.process_id,
            &stores.replay_events(),
        )
        .unwrap();
        assert_eq!(replayed.projection, completed.projection);
        assert!(replayed.next_action.is_none());
        assert_eq!(stores.replay_events::<LinearSagaEvent>().len(), 4);
        assert!(
            run(stores.store.commit(&retry_commit))
                .unwrap()
                .duplicate_input
        );
        let remaining = OutboxClaimRequest::new(
            stores.scope.clone(),
            id("pri_timer_host"),
            NonZeroU16::new(8).unwrap(),
        )
        .unwrap();
        assert!(
            run(claim_outbox(
                &policy,
                &stores.store,
                &remaining,
                LogicalTime(1)
            ))
            .unwrap()
            .is_empty()
        );
    }
}
