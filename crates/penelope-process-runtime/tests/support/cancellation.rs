//! A completed real worker cannot overwrite a concurrently canceled saga.
use super::*;
use penelope_executor::composition::TaskCompositionError;
use std::sync::Barrier;

fn batch(
    stores: &Stores,
    definition: &LinearSagaDefinition,
    projection: Option<&LinearSagaProjection>,
    input: &LinearSagaInput,
    sequence: u64,
    accepted: Option<ProcessInput>,
) -> (SagaDecision, AtomicProcessCommit) {
    let event = input.to_event(definition);
    let decision = decide(
        definition,
        projection,
        stores.scope.tenant_id.clone(),
        stores.scope.process_id.clone(),
        input,
    )
    .unwrap();
    let digest = stores.persist(&event);
    let input_id = event.input_id().unwrap();
    let accepted = accepted.unwrap_or_else(|| {
        ProcessInput::new(
            stores.scope.tenant_id.clone(),
            stores.scope.process_id.clone(),
            input_id.clone(),
            if matches!(input, LinearSagaInput::ManualResolution { .. }) {
                ProcessInputKind::ManualResolution
            } else {
                ProcessInputKind::CanonicalEvent
            },
            digest,
        )
    });
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
    let outcomes = decision
        .build_required_outcomes(&event, &stores.scope, sequence, &facts)
        .unwrap();
    let commit = AtomicProcessCommit::new(
        sequence,
        Some(accepted),
        outcomes,
        decision.next_action.clone().into_iter().collect(),
    )
    .unwrap();
    (decision, commit)
}

#[test]
fn completed_worker_relay_crossing_cancellation_cannot_resurrect_the_process() {
    for separate in [false, true] {
        let scope = scope();
        let definition = LinearSagaDefinition::new(
            scope.definition_id.clone(),
            scope.definition_version.clone(),
            scope.definition_digest,
            vec![step()],
        );
        let mut stores = Stores::new(separate, scope.clone());
        let (started, start_commit) = batch(
            &stores,
            &definition,
            None,
            &LinearSagaInput::Start {
                input_id: id("inp_start"),
                action_id: id("act_sum"),
            },
            0,
            Some(start_input(&scope)),
        );
        // Executes and reconstructs an actual protected worker to completion.
        // Its verified result has not yet entered the saga's durable inbox.
        let relay = stores.execute(&start_commit);
        let sequence = u64::try_from(start_commit.outcomes.len()).unwrap();
        let result_input = relay.linear_input(None);
        let (completed, stale_result) = batch(
            &stores,
            &definition,
            Some(&started.projection),
            &result_input,
            sequence,
            Some(relay.input().clone()),
        );
        assert_eq!(completed.projection.status, SagaStatus::Completed);
        let (escalated, unknown_commit) = batch(
            &stores,
            &definition,
            Some(&started.projection),
            &LinearSagaInput::ActionResult {
                input_id: id("inp_unknown"),
                observation: ActionResultObservation::unknown(id("act_sum")),
                next_action_id: None,
            },
            sequence,
            None,
        );
        run(stores.store.commit(&unknown_commit)).unwrap();
        assert_eq!(escalated.projection.status, SagaStatus::Escalated);
        let cancellation_sequence =
            sequence + u64::try_from(unknown_commit.outcomes.len()).unwrap();
        // The test fixture supplies an already-authorized operator decision;
        // application identity/authentication is outside this race test.
        let (canceled, cancel_commit) = batch(
            &stores,
            &definition,
            Some(&escalated.projection),
            &LinearSagaInput::ManualResolution {
                input_id: id("inp_cancel"),
                resolution: ManualReviewResolution::Cancel,
                next_action_id: None,
            },
            cancellation_sequence,
            None,
        );
        let result_store = SqliteProcessStore::open(
            &stores.process_path,
            SqliteLimits::default(),
            Arc::new(Clock(1)),
            100,
            8,
        )
        .unwrap();
        let cancel_store = SqliteProcessStore::open(
            &stores.process_path,
            SqliteLimits::default(),
            Arc::new(Clock(1)),
            100,
            8,
        )
        .unwrap();
        let barrier = Barrier::new(2);
        std::thread::scope(|threads| {
            let cancellation = threads.spawn(|| {
                barrier.wait();
                run(cancel_store.commit(&cancel_commit))
            });
            let completion = threads.spawn(|| {
                barrier.wait();
                run(relay.commit(&result_store, &stale_result))
            });
            cancellation.join().unwrap().unwrap();
            assert!(matches!(
                completion.join().unwrap(),
                Err(TaskCompositionError::Process(PortError::Conflict))
            ));
        });
        stores.reopen();
        let events: Vec<LinearSagaEvent> = stores.replay_events();
        assert_eq!(events.len(), 3);
        let reconstructed =
            replay(&definition, &scope.tenant_id, &scope.process_id, &events).unwrap();
        assert_eq!(reconstructed.projection, canceled.projection);
        assert_eq!(reconstructed.projection.status, SagaStatus::Cancelled);
        assert!(reconstructed.next_action.is_none());
        assert!(
            decide(
                &definition,
                Some(&reconstructed.projection),
                scope.tenant_id.clone(),
                scope.process_id.clone(),
                &result_input
            )
            .is_err()
        );
        assert!(matches!(
            run(relay.commit(&stores.store, &stale_result)),
            Err(TaskCompositionError::Process(PortError::Conflict))
        ));
        let page = run(stores.store.read_outcomes(
            &OutcomeReplayRequest::new(scope, 0, NonZeroU16::new(32).unwrap()).unwrap(),
        ))
        .unwrap();
        assert!(
            page.outcomes
                .iter()
                .any(|outcome| outcome.kind == ProcessOutcomeKind::Cancelled)
        );
        assert!(!page.outcomes.iter().any(|outcome| matches!(
            outcome.kind,
            ProcessOutcomeKind::ActionSucceeded | ProcessOutcomeKind::Completed
        )));
    }
}
