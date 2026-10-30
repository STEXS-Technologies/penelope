//! Real coordinator death between durable timer delivery operations.
use super::super::super::{cut, modes};
use super::*;
use std::{
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(in super::super) struct CrashPorts<'store>(pub(in super::super) &'store SqliteProcessStore);
#[async_trait]
impl TimerClaimStore for CrashPorts<'_> {
    async fn claim_due(&self, request: &TimerClaimRequest) -> Result<Vec<TimerLease>, PortError> {
        let leases = self.0.claim_due(request).await?;
        cut("timer-claim");
        Ok(leases)
    }
    async fn acknowledge(&self, lease: &TimerLease, now: LogicalTime) -> Result<(), PortError> {
        TimerClaimStore::acknowledge(self.0, lease, now).await?;
        cut("timer-ack");
        Ok(())
    }
}
#[async_trait]
impl ProcessPayloadStore for CrashPorts<'_> {
    async fn put_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
        bytes: &[u8],
    ) -> Result<ContentDigest, PortError> {
        let stored = self.0.put_payload(scope, digest, bytes).await?;
        cut("timer-payload");
        Ok(stored)
    }
    async fn read_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
    ) -> Result<Vec<u8>, PortError> {
        self.0.read_payload(scope, digest).await
    }
}
#[async_trait]
impl Inbox for CrashPorts<'_> {
    async fn accept(&self, input: &ProcessInput) -> Result<InboxAcceptanceReceipt, PortError> {
        let receipt = self.0.accept(input).await?;
        cut("timer-accept");
        Ok(receipt)
    }
}

#[test]
fn timer_delivery_reconstruction_consumes_one_input() {
    for separate in modes() {
        let stores = Stores::new(separate, scope());
        let mut step = super::super::super::step();
        step.retry_policy = RetryPolicy::new(NonZeroU32::new(2).unwrap()).with_backoff(
            RetryBackoff::new(NonZeroU64::new(10).unwrap(), NonZeroU64::new(10).unwrap()).unwrap(),
        );
        let definition = LinearSagaDefinition::new(
            stores.scope.definition_id.clone(),
            stores.scope.definition_version.clone(),
            stores.scope.definition_digest,
            vec![step],
        );
        let initial_history = stores.replay_events::<LinearSagaEvent>();
        if initial_history.is_empty() {
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
            acknowledge_dispatch(&stores, &original);
            let (waiting, _) = commit_decision(
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
            let timer = waiting.retry_timer_schedule().unwrap();
            run(stores.store.schedule(&timer)).unwrap();
            acknowledge_dispatch(&stores, &timer.action);
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
        let delivered_history = stores.replay_events::<LinearSagaEvent>();
        if delivered_history.len() == 2 {
            let waiting = replay(
                &definition,
                &stores.scope.tenant_id,
                &stores.scope.process_id,
                &delivered_history,
            )
            .unwrap();
            let timer = waiting.retry_timer_schedule().unwrap();
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
            let (_, commit) = commit_decision(
                &stores,
                &definition,
                &LinearSagaInput::RetryTimerFired {
                    input_id: accepted.input_id.clone(),
                    timer_action_id: timer.action.action_id,
                    fired_at: LogicalTime(10),
                    next_action_id: id("act_retry"),
                },
                Some(accepted.clone()),
            );
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
        let committed_history = stores.replay_events::<LinearSagaEvent>();
        assert_eq!(committed_history.len(), 3);
        let retried = replay(
            &definition,
            &stores.scope.tenant_id,
            &stores.scope.process_id,
            &committed_history,
        )
        .unwrap();
        let action = retried.next_action.unwrap();
        assert_eq!(action.action_id, id("act_retry"));
        assert_eq!(action.attempt, 1);
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

#[test]
fn killed_timer_coordinators_recover_delivery_and_decision() {
    for test in [
        "sagas::services::timer_saga::timer_crash::timer_delivery_reconstruction_consumes_one_input",
        "sagas::services::graph_timer::timer_delivery_reconstruction_consumes_one_input",
    ] {
        for separate in ["0", "1"] {
            for point in [
                "timer-claim",
                "timer-payload",
                "timer-accept",
                "timer-ack",
                "timer-decision",
            ] {
                let directory = tempfile::tempdir().unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", test, "--nocapture"])
                    .env("PENELOPE_SAGA_CRASH_DIR", directory.path())
                    .env("PENELOPE_SAGA_SEPARATE", separate)
                    .env("PENELOPE_SAGA_CUT", point)
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while !directory.path().join("crash-ready").exists() {
                    if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
                        let _kill = child.kill();
                        let _wait = child.wait();
                        panic!(
                            "{test}/{separate}/{point}: timer coordinator never reached boundary"
                        );
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                child.kill().unwrap();
                assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
                let mut recovery = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", test, "--nocapture"])
                    .env("PENELOPE_SAGA_CRASH_DIR", directory.path())
                    .env("PENELOPE_SAGA_SEPARATE", separate)
                    .env("PENELOPE_SAGA_RECOVER", "1")
                    .env("PENELOPE_SAGA_EXPECT_REPLAY", point)
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap();
                let recovery_deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    if let Some(status) = recovery.try_wait().unwrap() {
                        assert!(
                            status.success(),
                            "{test}/{separate}/{point}: recovery failed"
                        );
                        break;
                    }
                    if Instant::now() >= recovery_deadline {
                        let _kill = recovery.kill();
                        let _wait = recovery.wait();
                        panic!("{test}/{separate}/{point}: recovery timed out");
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
}
