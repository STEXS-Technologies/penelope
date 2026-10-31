//! Public payload fixture shared by ordinary latency and allocation probes.
use penelope_domain::*;
use penelope_ports::{PortError, payloads::ProcessPayloadStore};
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
pub(super) type Error = Box<dyn std::error::Error>;
pub(super) struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
pub(super) fn block_on<T>(future: impl Future<Output = T>) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or("deadline overflow")?;
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return Ok(result);
        }
        if Instant::now() >= deadline {
            return Err("payload operation timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
#[derive(Clone, Copy)]
pub(super) enum Case {
    Duplicate,
    Read,
    Missing,
    CorruptRead,
    CorruptPut,
}
impl Case {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::Read => "read",
            Self::Missing => "missing",
            Self::CorruptRead => "corrupt_read",
            Self::CorruptPut => "corrupt_put",
        }
    }
}
pub(super) enum Output {
    Digest(ContentDigest),
    Bytes(Vec<u8>),
}
pub(super) struct Fixture {
    _directory: tempfile::TempDir,
    store: SqliteProcessStore,
    connection: rusqlite::Connection,
    scope: ProcessScope,
    digest: ContentDigest,
    bytes: Vec<u8>,
    usage: i64,
    damaged: bool,
}
impl Fixture {
    fn new(size: usize) -> Result<Self, Error> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("payloads.db");
        let store =
            SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 10, 16)?;
        let scope = ProcessScope::new(
            "tnt_bench".parse()?,
            "prc_bench".parse()?,
            "def_bench".parse()?,
            "dfv_one".parse()?,
            ContentDigest([1; 32]),
        );
        let bytes = vec![7; size];
        let digest = ContentDigest::sha256(&bytes);
        if digest == ContentDigest([0; 32]) {
            return Err("missing-digest fixture collision".into());
        }
        if block_on(store.put_payload(&scope, digest, &bytes))?? != digest {
            return Err("wrong seed digest".into());
        }
        let connection = rusqlite::Connection::open(path)?;
        let usage =
            connection.query_row("SELECT bytes_used FROM process_usage", [], |row| row.get(0))?;
        Ok(Self {
            _directory: directory,
            store,
            connection,
            scope,
            digest,
            bytes,
            usage,
            damaged: false,
        })
    }
    pub(super) fn size(&self) -> usize {
        self.bytes.len()
    }
    pub(super) fn removed_bytes(&self, case: Case) -> usize {
        match case {
            Case::Duplicate => self.size(),
            Case::CorruptRead | Case::CorruptPut => self.size().max(1),
            Case::Read | Case::Missing => 0,
        }
    }
    pub(super) fn call(&self, case: Case) -> Result<Result<Output, PortError>, Error> {
        match case {
            Case::Duplicate | Case::CorruptPut => Ok(block_on(self.store.put_payload(
                &self.scope,
                self.digest,
                &self.bytes,
            ))?
            .map(Output::Digest)),
            Case::Read | Case::CorruptRead => {
                Ok(block_on(self.store.read_payload(&self.scope, self.digest))?.map(Output::Bytes))
            }
            Case::Missing => Ok(block_on(
                self.store.read_payload(&self.scope, ContentDigest([0; 32])),
            )?
            .map(Output::Bytes)),
        }
    }
    pub(super) fn validate(
        &self,
        case: Case,
        result: Result<Output, PortError>,
    ) -> Result<(), Error> {
        match (case, result) {
            (Case::Duplicate, Ok(Output::Digest(digest))) if digest == self.digest => Ok(()),
            (Case::Read, Ok(Output::Bytes(bytes))) if bytes == self.bytes => Ok(()),
            (Case::Missing | Case::CorruptRead | Case::CorruptPut, Err(PortError::Invariant)) => {
                Ok(())
            }
            _ => Err("incorrect payload result".into()),
        }
    }
    fn corrupt(&mut self) -> Result<(), Error> {
        self.connection.execute(
            "UPDATE process_payloads SET bytes=?1",
            [vec![8; self.size().max(1)]],
        )?;
        self.damaged = true;
        Ok(())
    }
    fn check_storage(&self) -> Result<(), Error> {
        let stored: Vec<u8> =
            self.connection
                .query_row("SELECT bytes FROM process_payloads", [], |row| row.get(0))?;
        let usage: i64 =
            self.connection
                .query_row("SELECT bytes_used FROM process_usage", [], |row| row.get(0))?;
        let expected = if self.damaged {
            vec![8; self.size().max(1)]
        } else {
            self.bytes.clone()
        };
        if usage != self.usage || stored != expected {
            return Err("payload operation changed rows or usage".into());
        }
        Ok(())
    }
}
pub(super) fn run(
    mut measure: impl FnMut(&Fixture, Case) -> Result<(), Error>,
) -> Result<(), Error> {
    for size in [0_usize, 32, 4096, 1048576] {
        let mut fixture = Fixture::new(size)?;
        for case in [
            Case::Duplicate,
            Case::Read,
            Case::Missing,
            Case::CorruptRead,
            Case::CorruptPut,
        ] {
            if matches!(case, Case::CorruptRead) {
                fixture.corrupt()?;
            }
            measure(&fixture, case)?;
            fixture.check_storage()?;
        }
        fixture
            .connection
            .execute("UPDATE process_payloads SET bytes=?1", [&fixture.bytes])?;
        fixture.damaged = false;
        fixture.validate(Case::Read, fixture.call(Case::Read)?)?;
        fixture.validate(Case::Duplicate, fixture.call(Case::Duplicate)?)?;
        fixture.check_storage()?;
    }
    Ok(())
}
