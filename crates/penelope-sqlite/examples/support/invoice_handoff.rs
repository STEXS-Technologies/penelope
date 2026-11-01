//! Consumer composition: durable command bytes precede a v0.3 atomic inbox/outbox
//! commit. Independent endpoint receipts make redelivery idempotent in this demo.
use super::invoice_model::{self, Invoice};
use penelope_core::task::Digest;
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::*;
use penelope_timeline::*;
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::{
    fmt::Write,
    future::Future,
    path::Path,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
// The example is a synchronous composition root, not an async-reactor hot path.
pub(super) fn wait<T>(future: impl Future<Output = Result<T, PortError>>) -> Result<T, PortError> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(15))
        .ok_or(PortError::TimedOut)?;
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return result;
        }
        if Instant::now() >= deadline {
            return Err(PortError::TimedOut);
        }
        std::thread::park_timeout(Duration::from_millis(5));
    }
}
pub(super) struct Clock {
    pub(super) now: AtomicU64,
}
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.now.load(Ordering::SeqCst)))
    }
}
const fn sql_failure(error: &rusqlite::Error) -> PortError {
    if matches!(error, rusqlite::Error::InvalidQuery) {
        PortError::Invariant
    } else {
        PortError::Unavailable
    }
}
fn connection(path: &Path) -> Result<Connection, PortError> {
    let connection = Connection::open(path).map_err(|error| sql_failure(&error))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| sql_failure(&error))?;
    let mode: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .map_err(|error| sql_failure(&error))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(PortError::Unavailable);
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|error| sql_failure(&error))?;
    connection
        .pragma_update(None, "max_page_count", 4096)
        .map_err(|error| sql_failure(&error))?;
    Ok(connection)
}
fn fixed<const BYTES: usize>(row: &Row<'_>, column: usize) -> rusqlite::Result<[u8; BYTES]> {
    row.get_ref(column)?
        .as_blob()
        .map_err(|_error| rusqlite::Error::InvalidQuery)?
        .try_into()
        .map_err(|_error| rusqlite::Error::InvalidQuery)
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Command {
    origin: [u8; 245],
    payload: [u8; 16],
}
impl Command {
    fn binding(self) -> Digest {
        let mut bytes = [0; 261];
        let (origin, payload) = bytes.split_at_mut(245);
        origin.copy_from_slice(&self.origin);
        payload.copy_from_slice(&self.payload);
        Digest::of(&bytes)
    }
    fn decode(self) -> Result<(TimelineIntentRecord, Invoice), PortError> {
        let TimelineRecord::Intent(record) =
            TimelineRecord::decode(&self.origin).map_err(|_error| PortError::Invariant)?
        else {
            return Err(PortError::Invariant);
        };
        let invoice = Invoice::from_bytes(self.payload).map_err(|_error| PortError::Invariant)?;
        if record.scope != invoice_model::scope()
            || record.digest != invoice.digest()
            || record.payload_bytes != invoice.byte_len()
        {
            return Err(PortError::Invariant);
        }
        Ok((record, invoice))
    }
}
// Stable across retries; origin branch/revision belongs in the content binding,
// not the dedup key. A conflicting re-admission under this same key fails closed.
fn key(record: TimelineIntentRecord) -> Digest {
    let mut bytes = Vec::with_capacity(160);
    for digest in [
        record.scope.tenant.digest(),
        record.scope.timeline.digest(),
        record.scope.model.digest(),
        record.scope.schema.digest(),
        record.identity.digest(),
    ] {
        bytes.extend_from_slice(&digest.0);
    }
    Digest::of(&bytes)
}
fn id<T: FromStr>(text: &str) -> Result<T, PortError> {
    text.parse().map_err(|_error| PortError::Invariant)
}
fn named<T: FromStr>(prefix: &str, digest: Digest) -> Result<T, PortError> {
    let mut value = format!("{prefix}_");
    for byte in digest.0 {
        write!(value, "{byte:02x}").map_err(|_error| PortError::Invariant)?;
    }
    id(&value)
}
fn batch(command: Command) -> Result<AtomicProcessCommit, PortError> {
    let (record, _invoice) = command.decode()?;
    let stable = key(record);
    let scope = ProcessScope::new(
        named("tnt", record.scope.tenant.digest())?,
        named("prc", stable)?,
        id("def_timeline_invoice")?,
        id("dfv_v1")?,
        ContentDigest(record.scope.model.digest().0),
    );
    // The application's explicit finality authorization is a manual-resolution
    // input; it is neither a fabricated canonical event nor a task result.
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        named("inp", stable)?,
        ProcessInputKind::ManualResolution,
        ContentDigest(command.binding().0),
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            named("out", stable)?,
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(100), // Injected saga admission time, independent of timeline steps.
            ProcessOutcomeKind::ActionPlanned,
            ContentDigest(command.binding().0),
        ),
    );
    let action = ProcessAction::new(
        scope,
        named("act", stable)?,
        id("stp_publish_invoice")?,
        0,
        ProcessActionKind::ExternalEffect,
        ContentDigest(command.binding().0),
    );
    AtomicProcessCommit::new(0, Some(input), vec![outcome], vec![action])
        .map_err(|_error| PortError::Invariant)
}

pub(super) struct Commands {
    connection: Connection,
    capacity: i64,
}
impl Commands {
    fn open(path: &Path) -> Result<Self, PortError> {
        let connection = connection(path)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS commands (
            effect_key BLOB PRIMARY KEY CHECK(length(effect_key)=32),
            binding BLOB NOT NULL UNIQUE CHECK(length(binding)=32),
            origin BLOB NOT NULL CHECK(length(origin)=245),
            payload BLOB NOT NULL CHECK(length(payload)=16))",
            )
            .map_err(|error| sql_failure(&error))?;
        Ok(Self {
            connection,
            capacity: 16,
        })
    }
    fn find(
        connection: &Connection,
        query: &str,
        digest: Digest,
    ) -> Result<Option<Command>, PortError> {
        connection
            .query_row(query, params![digest.0.as_slice()], |row| {
                let command = Command {
                    origin: fixed(row, 0)?,
                    payload: fixed(row, 1)?,
                };
                if command.binding().0 != fixed::<32>(row, 2)? {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                Ok(command)
            })
            .optional()
            .map_err(|error| sql_failure(&error))
    }
    fn save(&mut self, command: Command) -> Result<(), PortError> {
        let (record, _invoice) = command.decode()?;
        let stable = key(record);
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| sql_failure(&error))?;
        if let Some(existing) = Self::find(
            &transaction,
            "SELECT origin,payload,binding FROM commands WHERE effect_key=?1",
            stable,
        )? {
            if existing != command {
                return Err(PortError::Conflict);
            }
        } else {
            let count: i64 = transaction
                .query_row("SELECT count(*) FROM commands", [], |row| row.get(0))
                .map_err(|error| sql_failure(&error))?;
            if count >= self.capacity {
                return Err(PortError::QuotaExceeded);
            }
            transaction
                .execute(
                    "INSERT INTO commands VALUES (?1,?2,?3,?4)",
                    params![
                        stable.0.as_slice(),
                        command.binding().0.as_slice(),
                        command.origin.as_slice(),
                        command.payload.as_slice()
                    ],
                )
                .map_err(|error| sql_failure(&error))?;
        }
        transaction.commit().map_err(|error| sql_failure(&error))
    }
    fn load(&self, binding: Digest) -> Result<Command, PortError> {
        Self::find(
            &self.connection,
            "SELECT origin,payload,binding FROM commands WHERE binding=?1",
            binding,
        )?
        .ok_or(PortError::Invariant)
    }
    pub(super) fn len(&self) -> Result<i64, PortError> {
        self.connection
            .query_row("SELECT count(*) FROM commands", [], |row| row.get(0))
            .map_err(|error| sql_failure(&error))
    }
}
pub(super) struct Bridge {
    pub(super) store: SqliteProcessStore,
    pub(super) commands: Commands,
    lose_commit_ack: bool,
    pub(super) attempts: u64,
}
impl Bridge {
    pub(super) fn open(
        root: &Path,
        clock: Arc<Clock>,
        lose_commit_ack: bool,
    ) -> Result<Self, PortError> {
        Ok(Self {
            store: SqliteProcessStore::open(
                &root.join("process.db"),
                SqliteLimits::default(),
                clock,
                10,
                16,
            )?,
            commands: Commands::open(&root.join("commands.db"))?,
            lose_commit_ack,
            attempts: 0,
        })
    }
    pub(super) fn scope_for(&self, binding: Digest) -> Result<ProcessScope, PortError> {
        batch(self.commands.load(binding)?)?
            .outcomes
            .first()
            .map(ProcessOutcome::scope)
            .ok_or(PortError::Invariant)
    }
}
impl EffectHandoff<Invoice> for Bridge {
    type Error = PortError;
    fn submit(
        &mut self,
        effect: EligibleEffect<'_, Invoice>,
    ) -> Result<EffectAdmissionReceipt, PortError> {
        self.attempts = self
            .attempts
            .checked_add(1)
            .ok_or(PortError::QuotaExceeded)?;
        let descriptor = effect.descriptor();
        let mut origin = [0; 245];
        TimelineRecord::Intent(descriptor.record())
            .encode(&mut origin)
            .map_err(|_error| PortError::Invariant)?;
        let command = Command {
            origin,
            payload: effect.payload().bytes(),
        };
        // Durable, immutable payload upload comes first. A failed source commit
        // may leave a bounded orphan; no committed action can reference absent bytes.
        self.commands.save(command)?;
        let commit = batch(command)?;
        // Exact retry performs authoritative immutable batch verification inside
        // the SQLite adapter before acknowledging an existing commit. No new sequence.
        let receipt = wait(ProcessStore::commit(&self.store, &commit))?;
        receipt
            .validate_for(&commit)
            .map_err(|_error| PortError::Invariant)?;
        if self.lose_commit_ack {
            self.lose_commit_ack = false;
            return Err(PortError::Unavailable); // Inject loss AFTER the durable commit.
        }
        let scope = commit.outcomes.first().ok_or(PortError::Invariant)?.scope();
        let request = OutcomeReplayRequest::new(scope, 0, std::num::NonZeroU16::MIN)
            .map_err(|_error| PortError::Invariant)?;
        let page = wait(self.store.read_outcomes(&request))?;
        page.validate().map_err(|_error| PortError::Invariant)?;
        if page.outcomes != commit.outcomes || page.next_sequence.is_some() {
            return Err(PortError::Invariant);
        }
        let digest =
            Digest::of(&serde_json::to_vec(&commit).map_err(|_error| PortError::Invariant)?);
        Ok(EffectAdmissionReceipt {
            descriptor,
            durable_commit: digest,
        })
    }
}

/// A cooperating durable endpoint, deliberately separate from the source outbox.
/// Its idempotency transaction is part of this example's delivery contract.
pub(super) struct Destination {
    connection: Connection,
    capacity: i64,
}
impl Destination {
    pub(super) fn open(root: &Path) -> Result<Self, PortError> {
        let connection = connection(&root.join("destination.db"))?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS receipts (
            effect_key BLOB PRIMARY KEY CHECK(length(effect_key)=32),
            binding BLOB NOT NULL CHECK(length(binding)=32),
            payload BLOB NOT NULL CHECK(length(payload)=16))",
            )
            .map_err(|error| sql_failure(&error))?;
        Ok(Self {
            connection,
            capacity: 16,
        })
    }
    pub(super) fn deliver(
        &mut self,
        action: &ProcessAction,
        commands: &Commands,
    ) -> Result<bool, PortError> {
        let command = commands.load(Digest(action.payload_digest.0))?;
        let (record, _invoice) = command.decode()?;
        let commit = batch(command)?;
        if commit.actions.first() != Some(action) {
            return Err(PortError::Invariant);
        }
        let stable = key(record);
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| sql_failure(&error))?;
        let old = transaction
            .query_row(
                "SELECT binding,payload FROM receipts WHERE effect_key=?1",
                params![stable.0.as_slice()],
                |row| Ok((fixed::<32>(row, 0)?, fixed::<16>(row, 1)?)),
            )
            .optional()
            .map_err(|error| sql_failure(&error))?;
        let duplicate = if let Some((binding, payload)) = old {
            if binding != command.binding().0 || payload != command.payload {
                return Err(PortError::Conflict);
            }
            true
        } else {
            let count: i64 = transaction
                .query_row("SELECT count(*) FROM receipts", [], |row| row.get(0))
                .map_err(|error| sql_failure(&error))?;
            if count >= self.capacity {
                return Err(PortError::QuotaExceeded);
            }
            transaction
                .execute(
                    "INSERT INTO receipts VALUES (?1,?2,?3)",
                    params![
                        stable.0.as_slice(),
                        command.binding().0.as_slice(),
                        command.payload.as_slice()
                    ],
                )
                .map_err(|error| sql_failure(&error))?;
            false
        };
        transaction.commit().map_err(|error| sql_failure(&error))?;
        Ok(duplicate)
    }
    pub(super) fn invoices(&self) -> Result<Vec<Invoice>, PortError> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM receipts LIMIT 17")
            .map_err(|error| sql_failure(&error))?;
        let mut rows = statement.query([]).map_err(|error| sql_failure(&error))?;
        let mut result = Vec::new();
        while let Some(row) = rows.next().map_err(|error| sql_failure(&error))? {
            if result.len() >= 16 {
                return Err(PortError::QuotaExceeded);
            }
            result.push(
                Invoice::from_bytes(fixed(row, 0).map_err(|error| sql_failure(&error))?)
                    .map_err(|_error| PortError::Invariant)?,
            );
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command(invoice: Invoice, revision: u64) -> Result<Command, PortError> {
        let record = TimelineIntentRecord {
            scope: invoice_model::scope(),
            branch: TimelineBranchId::new(1),
            step: TimelineStep::new(3),
            revision: TimelineRevision::new(revision),
            identity: TimelineEffectId::from_digest(Digest::of(b"invoice-summary")),
            digest: invoice.digest(),
            payload_bytes: invoice.byte_len(),
        };
        let mut origin = [0; 245];
        TimelineRecord::Intent(record)
            .encode(&mut origin)
            .map_err(|_error| PortError::Invariant)?;
        Ok(Command {
            origin,
            payload: invoice.bytes(),
        })
    }
    #[test]
    fn immutable_payload_and_origin_binding_survive_reopen()
    -> Result<(), super::super::ExampleError> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("commands.db");
        let original = command(
            Invoice {
                total: 44,
                units: 6,
            },
            5,
        )?;
        let mut commands = Commands::open(&path)?;
        commands.save(original)?;
        commands.save(original)?;
        assert_eq!(commands.len()?, 1);
        let changed_origin = command(
            Invoice {
                total: 44,
                units: 6,
            },
            6,
        )?;
        assert_eq!(commands.save(changed_origin), Err(PortError::Conflict));
        let changed_payload = command(
            Invoice {
                total: 45,
                units: 6,
            },
            5,
        )?;
        assert_eq!(commands.save(changed_payload), Err(PortError::Conflict));
        drop(commands);
        let reopened = Commands::open(&path)?;
        let restored = reopened.load(original.binding())?;
        assert!(restored == original);
        let mut corrupt = original;
        corrupt.payload = Invoice {
            total: 45,
            units: 6,
        }
        .bytes();
        assert!(matches!(corrupt.decode(), Err(PortError::Invariant)));
        reopened
            .connection
            .execute(
                "UPDATE commands SET payload=?1",
                params![corrupt.payload.as_slice()],
            )
            .map_err(|error| sql_failure(&error))?;
        assert!(reopened.load(original.binding()).is_err());
        Ok(())
    }
    #[test]
    fn command_capacity_and_wrong_scope_are_refused_before_admission()
    -> Result<(), super::super::ExampleError> {
        let directory = tempfile::tempdir()?;
        let mut commands = Commands::open(&directory.path().join("commands.db"))?;
        commands.capacity = 0;
        let original = command(
            Invoice {
                total: 44,
                units: 6,
            },
            5,
        )?;
        assert_eq!(commands.save(original), Err(PortError::QuotaExceeded));
        assert_eq!(commands.len()?, 0);
        let (mut record, _invoice) = original.decode()?;
        record.scope.tenant = TimelineTenantId::from_digest(Digest([99; 32]));
        let mut foreign = original;
        TimelineRecord::Intent(record)
            .encode(&mut foreign.origin)
            .map_err(|_error| PortError::Invariant)?;
        assert_eq!(commands.save(foreign), Err(PortError::Invariant));
        assert_eq!(commands.len()?, 0);
        Ok(())
    }
    #[test]
    fn destination_rejects_substituted_action_identity() -> Result<(), super::super::ExampleError> {
        let directory = tempfile::tempdir()?;
        let original = command(
            Invoice {
                total: 44,
                units: 6,
            },
            5,
        )?;
        let mut commands = Commands::open(&directory.path().join("commands.db"))?;
        commands.save(original)?;
        let commit = batch(original)?;
        let mut action = commit.actions.first().ok_or(PortError::Invariant)?.clone();
        action.action_id = id("act_substitute")?;
        let mut destination = Destination::open(directory.path())?;
        assert_eq!(
            destination.deliver(&action, &commands),
            Err(PortError::Invariant)
        );
        assert!(destination.invoices()?.is_empty());
        Ok(())
    }
}
