use super::{Admission, Error, id};
use penelope_domain::*;
use penelope_executor::engine::*;
use penelope_ports::{payloads::ProcessPayloadStore, *};
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    num::NonZeroU16,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(future: impl Future<Output = Result<T, PortError>>) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(30))
        .ok_or("deadline overflow")?;
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return Ok(result?);
        }
        if Instant::now() >= deadline {
            return Err("database operation timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Recorded {
    event: LinearSagaEvent,
    result: Option<ContentDigest>,
}
pub struct Storage {
    directory: tempfile::TempDir,
    store: Option<SqliteProcessStore>,
    pending: Option<OutboxLease>,
    next_sequence: u64,
    admission: Admission,
}
impl Storage {
    pub fn new(admission: Admission) -> Result<Self, Error> {
        let directory = tempfile::tempdir()?;
        let store = SqliteProcessStore::open(
            &directory.path().join("workflow.db"),
            SqliteLimits::default(),
            Arc::new(Clock),
            30_000,
            16,
        )?;
        Ok(Self {
            directory,
            store: Some(store),
            pending: None,
            next_sequence: 0,
            admission,
        })
    }
    fn store(&self) -> Result<&SqliteProcessStore, Error> {
        self.store.as_ref().ok_or_else(|| "store closed".into())
    }
    pub fn reopen(&mut self) -> Result<(), Error> {
        drop(self.store.take());
        self.next_sequence = 0;
        self.store = Some(SqliteProcessStore::open(
            &self.directory.path().join("workflow.db"),
            SqliteLimits::default(),
            Arc::new(Clock),
            30_000,
            16,
        )?);
        Ok(())
    }
    pub fn commit(
        &mut self,
        scope: &ProcessScope,
        decision: &SagaDecision,
        event: &LinearSagaEvent,
        value: Option<u64>,
    ) -> Result<(), Error> {
        if matches!(event, LinearSagaEvent::Started { .. }) {
            self.next_sequence = 0;
        }
        let sequence = self.next_sequence;
        let store = self.store()?;
        let bytes = serde_json::to_vec(&Recorded {
            event: event.clone(),
            result: value.map(|value| ContentDigest::sha256(&value.to_le_bytes())),
        })?;
        let digest = ContentDigest::sha256(&bytes);
        if wait(store.put_payload(scope, digest, &bytes))? != digest {
            return Err("event payload receipt differs".into());
        }
        let input_id = event.input_id().ok_or("event has no input identity")?;
        let input = ProcessInput::new(
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            input_id.clone(),
            ProcessInputKind::CanonicalEvent,
            digest,
        );
        if self.admission == Admission::Preaccepted {
            let accepted = wait(store.accept(&input))?;
            accepted.validate_for(&input)?;
        }
        let mut facts = Vec::new();
        for (offset, kind) in event
            .observed_outcome_kinds()
            .iter()
            .chain(decision.planned_outcome_kinds())
            .enumerate()
        {
            let number = sequence + u64::try_from(offset)?;
            facts.push(ProcessOutcomeFact::new(
                id(&format!("out_{number}"))?,
                CausationId::Input(input_id.clone()),
                OutcomeActor::System,
                LogicalTime(100),
                *kind,
                digest,
            ));
        }
        let outcomes = decision.build_required_outcomes(event, scope, sequence, &facts)?;
        let commit = AtomicProcessCommit::new(
            sequence,
            Some(input),
            outcomes,
            decision.next_action.iter().cloned().collect(),
        )?;
        let receipt = wait(store.commit(&commit))?;
        receipt.validate_for(&commit)?;
        if receipt.duplicate_input != (self.admission == Admission::Preaccepted) {
            return Err("unexpected input admission receipt".into());
        }
        self.next_sequence = receipt.committed_through_sequence + 1;
        Ok(())
    }
    pub fn claim(&mut self, scope: &ProcessScope, expected: &ProcessAction) -> Result<(), Error> {
        let request = OutboxClaimRequest::new(
            scope.clone(),
            id("pri_workload")?,
            NonZeroU16::new(1).ok_or("claim bound")?,
        )?;
        let mut leases = wait(self.store()?.claim(&request))?;
        if leases.len() != 1 || leases.first().ok_or("missing lease")?.record.action != *expected {
            return Err("claimed action differs".into());
        }
        self.pending = leases.pop();
        // Keep the lease until its matching result is durably recorded.
        Ok(())
    }
    pub fn result(
        &self,
        scope: &ProcessScope,
        action: &ProcessAction,
        value: u64,
    ) -> Result<(), Error> {
        let store = self.store()?;
        let bytes = value.to_le_bytes();
        let digest = ContentDigest::sha256(&bytes);
        if wait(store.put_payload(scope, digest, &bytes))? != digest
            || wait(store.read_payload(scope, digest))? != bytes
        {
            return Err("business result payload differs".into());
        }
        if self
            .pending
            .as_ref()
            .ok_or("missing pending lease")?
            .record
            .action
            != *action
        {
            return Err("pending lease action differs".into());
        }
        Ok(())
    }
    pub fn acknowledge(&mut self) -> Result<(), Error> {
        let lease = self.pending.take().ok_or("missing pending lease")?;
        wait(OutboxStore::acknowledge(
            self.store()?,
            &lease,
            LogicalTime(100),
        ))?;
        Ok(())
    }
    pub fn events(
        &mut self,
        scope: &ProcessScope,
    ) -> Result<(Vec<LinearSagaEvent>, Option<u64>), Error> {
        let store = self.store()?;
        let mut from = 0;
        let mut events = Vec::new();
        let mut frontier = 0;
        let mut value = None;
        loop {
            let request = OutcomeReplayRequest::new(
                scope.clone(),
                from,
                NonZeroU16::new(16).ok_or("page bound")?,
            )?;
            let page = wait(store.read_outcomes(&request))?;
            for outcome in page.outcomes {
                frontier = outcome.sequence + 1;
                if outcome.kind == ProcessOutcomeKind::InputAccepted {
                    let bytes = wait(store.read_payload(scope, outcome.payload_digest))?;
                    let recorded: Recorded = serde_json::from_slice(&bytes)?;
                    if let Some(digest) = recorded.result {
                        let result = wait(store.read_payload(scope, digest))?;
                        value =
                            Some(u64::from_le_bytes(result.try_into().map_err(
                                |_length| "invalid durable business result length",
                            )?));
                    }
                    events.push(recorded.event);
                }
            }
            if let Some(next) = page.next_sequence {
                if next <= from {
                    return Err("replay did not advance".into());
                }
                from = next;
            } else {
                break;
            }
        }
        self.next_sequence = frontier;
        Ok((events, value))
    }
    pub fn check_finished(&self, scope: &ProcessScope) -> Result<(), Error> {
        let request = OutboxClaimRequest::new(
            scope.clone(),
            id("pri_workload")?,
            NonZeroU16::new(1).ok_or("claim bound")?,
        )?;
        if !wait(self.store()?.claim(&request))?.is_empty() {
            return Err("unacknowledged action remains".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn recovery_rejects_missing_business_result_evidence() -> Result<(), super::Error> {
        use super::*;
        let mut fixture = crate::workload::Fixture::new(crate::workload::Case::Durable)?;
        let scope = ProcessScope::new(
            id("tnt_workload")?,
            id("prc_missing_evidence")?,
            fixture.linear.definition_id.clone(),
            fixture.linear.definition_version.clone(),
            fixture.linear.definition_digest,
        );
        let input = LinearSagaInput::Start {
            input_id: id("inp_start")?,
            action_id: id("act_0")?,
        };
        let decision = decide(
            &fixture.linear,
            None,
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            &input,
        )?;
        let storage = fixture.storage.as_mut().ok_or("missing durable fixture")?;
        // Deliberately reference output that was never put into the payload store.
        storage.commit(
            &scope,
            &decision,
            &input.to_event(&fixture.linear),
            Some(42),
        )?;
        storage.reopen()?;
        if storage.events(&scope).is_ok() {
            return Err("missing durable business result was accepted".into());
        }
        Ok(())
    }
    #[test]
    fn admission_modes_preserve_identical_completed_rows() -> Result<(), super::Error> {
        use super::*;
        fn snapshot(storage: &Storage) -> Result<Vec<Vec<Vec<rusqlite::types::Value>>>, Error> {
            let connection =
                rusqlite::Connection::open(storage.directory.path().join("workflow.db"))?;
            let mut result = Vec::new();
            for table in [
                "process_usage",
                "process_heads",
                "process_outcomes",
                "process_inbox",
                "process_sources",
                "process_commits",
                "process_actions",
                "process_timers",
                "process_payloads",
                "lease_counter",
            ] {
                let mut statement =
                    connection.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
                let columns = statement.column_count();
                let rows = statement
                    .query_map([], |row| {
                        (0..columns)
                            .map(|column| row.get(column))
                            .collect::<Result<Vec<rusqlite::types::Value>, _>>()
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                result.push(rows);
            }
            Ok(result)
        }
        let mut expected = None;
        for admission in [Admission::Preaccepted, Admission::Atomic] {
            let mut fixture = crate::workload::Fixture::with_admission(
                crate::workload::Case::DurableReplay,
                admission,
            )?;
            let result = fixture.run(crate::workload::Case::DurableReplay, 3, 64)?;
            let rows = snapshot(fixture.storage.as_ref().ok_or("missing storage")?)?;
            if let Some(previous) = &expected {
                if previous != &(result, rows.clone()) {
                    return Err("admission modes changed final rows or result".into());
                }
            } else {
                expected = Some((result, rows));
            }
        }
        Ok(())
    }
    #[test]
    fn failed_atomic_admission_rolls_back_input_while_preacceptance_remains_durable()
    -> Result<(), super::Error> {
        use super::*;
        for admission in [Admission::Preaccepted, Admission::Atomic] {
            let mut fixture = crate::workload::Fixture::with_admission(
                crate::workload::Case::Durable,
                admission,
            )?;
            let scope = ProcessScope::new(
                id("tnt_workload")?,
                id("prc_failed_admission")?,
                fixture.linear.definition_id.clone(),
                fixture.linear.definition_version.clone(),
                fixture.linear.definition_digest,
            );
            let input = LinearSagaInput::Start {
                input_id: id("inp_start")?,
                action_id: id("act_0")?,
            };
            let decision = decide(
                &fixture.linear,
                None,
                scope.tenant_id.clone(),
                scope.process_id.clone(),
                &input,
            )?;
            let event = input.to_event(&fixture.linear);
            let storage = fixture.storage.as_mut().ok_or("missing storage")?;
            let connection =
                rusqlite::Connection::open(storage.directory.path().join("workflow.db"))?;
            connection.execute_batch("CREATE TRIGGER reject_outcome BEFORE INSERT ON process_outcomes BEGIN SELECT RAISE(ABORT,'injected outcome failure'); END;")?;
            if storage.commit(&scope, &decision, &event, None).is_ok() {
                return Err("injected commit failure was ignored".into());
            }
            let counts:(u32,u32,u32,u32,u32) = connection.query_row("SELECT (SELECT count(*) FROM process_inbox),(SELECT count(*) FROM process_outcomes),(SELECT count(*) FROM process_commits),(SELECT count(*) FROM process_heads),(SELECT count(*) FROM process_actions)",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?;
            let expected_input = u32::from(admission == Admission::Preaccepted);
            if counts != (expected_input, 0, 0, 0, 0) {
                return Err("failed admission left unexpected rows".into());
            }
            storage.reopen()?;
            connection.execute_batch("DROP TRIGGER reject_outcome;")?;
            storage.commit(&scope, &decision, &event, None)?;
            let (events, value) = storage.events(&scope)?;
            if events != vec![event] || value.is_some() {
                return Err("reopened admission retry differs".into());
            }
            let consumed: u32 = connection.query_row(
                "SELECT count(*) FROM process_inbox WHERE commit_digest IS NOT NULL",
                [],
                |row| row.get(0),
            )?;
            if consumed != 1 {
                return Err("input was not consumed exactly once".into());
            }
        }
        Ok(())
    }
}
