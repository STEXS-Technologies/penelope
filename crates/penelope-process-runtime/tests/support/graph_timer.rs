//! Graph timer delivery uses explicit graph steps, preserving v1 action kinds.
use super::*;
use penelope_executor::{engine::*, graph::*};
use penelope_ports::{inbox::PendingInbox, payloads::ProcessPayloadStore};
use std::num::NonZeroU32;

fn prepare(
    stores: &Stores,
    definition: &ProcessGraphDefinition,
    input: GraphSagaInput,
    accepted: Option<ProcessInput>,
) -> (GraphSagaDecision, AtomicProcessCommit) {
    let history = stores.replay_events::<GraphSagaEvent>();
    let decision = if history.is_empty() {
        start_graph(
            definition,
            stores.scope.tenant_id.clone(),
            stores.scope.process_id.clone(),
            input,
        )
        .unwrap()
    } else {
        let running = replay_graph(
            definition,
            &stores.scope.tenant_id,
            &stores.scope.process_id,
            &history,
        )
        .unwrap();
        apply_graph_result(definition, &running, input).unwrap()
    };
    let digest = stores.persist(&decision.event);
    let page = run(stores.store.read_outcomes(
        &OutcomeReplayRequest::new(stores.scope.clone(), 0, NonZeroU16::new(32).unwrap()).unwrap(),
    ))
    .unwrap();
    assert!(page.next_sequence.is_none());
    let sequence = u64::try_from(page.outcomes.len()).unwrap();
    let input_id = decision.event.input_id();
    let facts = stores.facts(
        input_id,
        decision
            .event
            .observed_outcome_kinds()
            .iter()
            .chain(decision.planned_outcome_kinds())
            .copied(),
        digest,
        sequence,
    );
    let outcomes = decision
        .build_required_outcomes(&stores.scope, sequence, &facts)
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
        outcomes,
        decision.next_action.iter().cloned().collect(),
    )
    .unwrap();
    (decision, commit)
}
fn persist_commit(stores: &Stores, commit: &AtomicProcessCommit) {
    run(stores.store.commit(commit))
        .unwrap()
        .validate_for(commit)
        .unwrap();
}

#[test]
fn discovered_graph_timer_selects_one_retry_task_and_replays_terminal_state() {
    for separate in [false, true] {
        let mut stores = Stores::new(separate, scope());
        let definition = timer_definition(&stores);
        let (started, start_commit) = prepare(
            &stores,
            &definition,
            GraphSagaInput::Start {
                input_id: id("inp_start"),
                action_id: id("act_initial"),
            },
            None,
        );
        persist_commit(&stores, &start_commit);
        let original = started.next_action.unwrap();
        super::timer_saga::acknowledge_dispatch(&stores, &original);
        let (waiting, timer_commit) = prepare(
            &stores,
            &definition,
            GraphSagaInput::ActionResult {
                input_id: id("inp_failure"),
                observation: ActionResultObservation::retryable_failure(original.action_id),
                next_action_id: Some(id("act_timer")),
            },
            None,
        );
        persist_commit(&stores, &timer_commit);
        let timer = TimerSchedule {
            action: waiting.next_action.unwrap(),
            due_at: LogicalTime(10),
        };
        assert_eq!(timer.action.kind, ProcessActionKind::Timer);
        assert_eq!(
            timer.action.payload_digest,
            ContentDigest::sha256(&timer.due_at.0.to_be_bytes())
        );
        run(stores.store.schedule(&timer)).unwrap();
        super::timer_saga::acknowledge_dispatch(&stores, &timer.action);
        let policy = Policy {
            allow: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        };
        let request = TimerClaimRequest::new(
            stores.scope.clone(),
            id("pri_timers"),
            LogicalTime(9999),
            NonZeroU16::new(2).unwrap(),
        )
        .unwrap();
        let clock = Clock(AtomicU64::new(10));
        let delivered = run(drain_timers(
            &policy,
            &stores.store,
            &stores.store,
            &request,
            &clock,
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
        let canonical = timer.canonical_wire_bytes().unwrap();
        assert_eq!(accepted.payload_digest, ContentDigest::sha256(&canonical));
        assert_eq!(
            run(stores
                .store
                .read_payload(&stores.scope, accepted.payload_digest))
            .unwrap(),
            canonical
        );
        let replayed = replay_graph(
            &definition,
            &stores.scope.tenant_id,
            &stores.scope.process_id,
            &stores.replay_events(),
        )
        .unwrap();
        assert_eq!(replayed.scope, timer.action.scope());
        assert_eq!(
            replayed.active_action_id.as_ref(),
            Some(&timer.action.action_id)
        );
        let (retried, retry_commit) = prepare(
            &stores,
            &definition,
            GraphSagaInput::ActionResult {
                input_id: accepted.input_id.clone(),
                observation: ActionResultObservation::succeeded(timer.action.action_id),
                next_action_id: Some(id("act_retry")),
            },
            Some(accepted.clone()),
        );
        persist_commit(&stores, &retry_commit);
        assert_eq!(
            retried.next_action.as_ref().unwrap().step_id,
            id("stp_retry")
        );
        assert!(
            run(stores.store.pending_inputs(
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                NonZeroU16::new(2).unwrap()
            ))
            .unwrap()
            .is_empty()
        );
        assert!(
            run(stores.store.commit(&retry_commit))
                .unwrap()
                .duplicate_input
        );
        let relay = stores.execute(&retry_commit);
        let (completed, result_commit) = prepare(
            &stores,
            &definition,
            relay.graph_input(None),
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
        assert_eq!(
            replay_graph(
                &definition,
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                &stores.replay_events()
            )
            .unwrap(),
            completed.projection
        );
        assert_eq!(stores.replay_events::<GraphSagaEvent>().len(), 4);
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

fn timer_definition(stores: &Stores) -> ProcessGraphDefinition {
    let mut initial = super::super::step();
    initial.step_id = id("stp_initial");
    let mut delay = super::super::step();
    delay.step_id = id("stp_delay");
    delay.action_kind = ProcessActionKind::Timer;
    // This application's timer-step payload pins the absolute logical due
    // time. The host checks it before accepting the TimerFired observation.
    delay.payload_digest = ContentDigest::sha256(&10_u64.to_be_bytes());
    let mut retry = super::super::step();
    retry.step_id = id("stp_retry");
    ProcessGraphDefinition {
        definition_id: stores.scope.definition_id.clone(),
        definition_version: stores.scope.definition_version.clone(),
        definition_digest: stores.scope.definition_digest,
        entry_step_id: initial.step_id.clone(),
        transitions: vec![
            GraphTransition {
                from_step: initial.step_id.clone(),
                on: GraphTransitionOutcome::RetryableFailure,
                to_step: Some(delay.step_id.clone()),
            },
            GraphTransition {
                from_step: delay.step_id.clone(),
                on: GraphTransitionOutcome::Succeeded,
                to_step: Some(retry.step_id.clone()),
            },
            GraphTransition {
                from_step: retry.step_id.clone(),
                on: GraphTransitionOutcome::Succeeded,
                to_step: None,
            },
        ],
        steps: vec![initial, delay, retry],
        max_step_visits: NonZeroU32::new(3).unwrap(),
    }
}

#[test]
fn timer_delivery_reconstruction_consumes_one_input() {
    use super::super::{cut, modes};
    use super::timer_saga::timer_crash::CrashPorts;
    for separate in modes() {
        let stores = Stores::new(separate, scope());
        let definition = timer_definition(&stores);
        let initial_history = stores.replay_events::<GraphSagaEvent>();
        if initial_history.is_empty() {
            let (started, start) = prepare(
                &stores,
                &definition,
                GraphSagaInput::Start {
                    input_id: id("inp_start"),
                    action_id: id("act_initial"),
                },
                None,
            );
            persist_commit(&stores, &start);
            let original = started.next_action.unwrap();
            super::timer_saga::acknowledge_dispatch(&stores, &original);
            let (waiting, scheduled) = prepare(
                &stores,
                &definition,
                GraphSagaInput::ActionResult {
                    input_id: id("inp_failure"),
                    observation: ActionResultObservation::retryable_failure(original.action_id),
                    next_action_id: Some(id("act_timer")),
                },
                None,
            );
            persist_commit(&stores, &scheduled);
            let timer = TimerSchedule {
                action: waiting.next_action.unwrap(),
                due_at: LogicalTime(10),
            };
            run(stores.store.schedule(&timer)).unwrap();
            super::timer_saga::acknowledge_dispatch(&stores, &timer.action);
        } else {
            let point = std::env::var("PENELOPE_SAGA_EXPECT_REPLAY").unwrap();
            assert_eq!(
                initial_history.len(),
                if point == "timer-decision" { 3 } else { 2 }
            );
            let pending = run(stores.store.pending_inputs(
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                NonZeroU16::new(2).unwrap(),
            ))
            .unwrap();
            assert_eq!(
                pending.len(),
                usize::from(matches!(point.as_str(), "timer-accept" | "timer-ack"))
            );
        }
        let request = TimerClaimRequest::new(
            stores.scope.clone(),
            id("pri_timers"),
            LogicalTime(999999),
            NonZeroU16::new(2).unwrap(),
        )
        .unwrap();
        let policy = Policy {
            allow: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        };
        let ports = CrashPorts(&stores.store);
        let clock = Clock(AtomicU64::new(if stores.now.0 == 1 {
            10
        } else {
            stores.now.0
        }));
        let delivered = run(drain_timers(&policy, &ports, &ports, &request, &clock)).unwrap();
        if let Ok(point) = std::env::var("PENELOPE_SAGA_EXPECT_REPLAY") {
            assert_eq!(
                delivered.acknowledged,
                u16::from(!matches!(point.as_str(), "timer-ack" | "timer-decision"))
            );
            assert_eq!(
                delivered.duplicate_inputs,
                u16::from(point == "timer-accept")
            );
        }
        let delivered_history = stores.replay_events::<GraphSagaEvent>();
        if delivered_history.len() == 2 {
            let waiting = replay_graph(
                &definition,
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                &delivered_history,
            )
            .unwrap();
            let timer = TimerSchedule {
                action: ProcessAction::new(
                    stores.scope.clone(),
                    waiting.active_action_id.unwrap(),
                    id("stp_delay"),
                    waiting.step_visits.saturating_sub(1),
                    ProcessActionKind::Timer,
                    ContentDigest::sha256(&10_u64.to_be_bytes()),
                ),
                due_at: LogicalTime(10),
            };
            assert_eq!(
                timer.action.payload_digest,
                ContentDigest::sha256(&timer.due_at.0.to_be_bytes())
            );
            let pending = run(stores.store.pending_inputs(
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                NonZeroU16::new(2).unwrap(),
            ))
            .unwrap();
            assert_eq!(pending.len(), 1);
            let accepted = pending.first().unwrap();
            let bytes = timer.canonical_wire_bytes().unwrap();
            assert_eq!(accepted.kind, ProcessInputKind::TimerFired);
            assert_eq!(accepted.payload_digest, ContentDigest::sha256(&bytes));
            assert_eq!(
                run(stores
                    .store
                    .read_payload(&stores.scope, accepted.payload_digest))
                .unwrap(),
                bytes
            );
            let (_, commit) = prepare(
                &stores,
                &definition,
                GraphSagaInput::ActionResult {
                    input_id: accepted.input_id.clone(),
                    observation: ActionResultObservation::succeeded(timer.action.action_id),
                    next_action_id: Some(id("act_retry")),
                },
                Some(accepted.clone()),
            );
            persist_commit(&stores, &commit);
            cut("timer-decision");
            assert!(run(stores.store.commit(&commit)).unwrap().duplicate_input);
        }
        assert!(
            run(stores.store.pending_inputs(
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                NonZeroU16::new(2).unwrap()
            ))
            .unwrap()
            .is_empty()
        );
        let committed_history = stores.replay_events::<GraphSagaEvent>();
        assert_eq!(committed_history.len(), 3);
        let retried = replay_graph(
            &definition,
            &stores.scope.tenant_id,
            &stores.scope.process_id,
            &committed_history,
        )
        .unwrap();
        let action = ProcessAction::new(
            stores.scope.clone(),
            retried.active_action_id.unwrap(),
            id("stp_retry"),
            retried.step_visits.saturating_sub(1),
            ProcessActionKind::ExternalEffect,
            ContentDigest::sha256(&[0, 1, 2, 3]),
        );
        assert_eq!(action.action_id, id("act_retry"));
        assert_eq!(action.step_id, id("stp_retry"));
        let claim = OutboxClaimRequest::new(
            stores.scope.clone(),
            id("pri_retry"),
            NonZeroU16::new(8).unwrap(),
        )
        .unwrap();
        let leases = run(claim_outbox(&policy, &stores.store, &claim, stores.now)).unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases.first().unwrap().record.action, action);
    }
}
