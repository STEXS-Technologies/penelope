//! Kill a populated version-five upgrade before and after index/version changes.
use super::*;
use penelope_domain::{
    CausationId, ContentDigest, LogicalTime, OutcomeActor, ProcessInput, ProcessInputKind,
    ProcessOutcome, ProcessOutcomeFact, ProcessOutcomeKind, ProcessScope,
};
use penelope_ports::AtomicProcessCommit;
use rusqlite::{params, types::Value};
type Rows = Vec<Vec<Value>>;
fn rows(connection: &Connection) -> Vec<Rows> {
    [
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
    ]
    .iter()
    .map(|table| {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns)
                    .map(|column| row.get(column))
                    .collect::<rusqlite::Result<Vec<Value>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Rows>>()
            .unwrap()
    })
    .collect()
}
fn seed_v5(path: &Path) {
    let limits = SqliteLimits::default();
    let mut connection = open(path, limits).unwrap();
    let scope = ProcessScope::new(
        "tnt_migration".parse().unwrap(),
        "prc_migration".parse().unwrap(),
        "def_migration".parse().unwrap(),
        "dfv_one".parse().unwrap(),
        ContentDigest([1; 32]),
    );
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        "inp_consumed".parse().unwrap(),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([2; 32]),
    );
    let outcome = ProcessOutcome::new(
        scope,
        0,
        ProcessOutcomeFact::new(
            "out_start".parse().unwrap(),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::Started,
            ContentDigest([3; 32]),
        ),
    );
    let commit =
        AtomicProcessCommit::new(0, Some(input.clone()), vec![outcome], Vec::new()).unwrap();
    crate::process::commit_process(&mut connection, &commit, limits).unwrap();
    // Synthetic admission for valid pending inputs, including the normal usage
    // charge. The public-port migration test separately covers admission itself.
    let key = crate::process::process_key(&input).unwrap();
    let transaction = connection.transaction().unwrap();
    for index in 0..128 {
        let mut pending = input.clone();
        pending.input_id = format!("inp_pending_{index:03}").parse().unwrap();
        let bytes = serde_json::to_vec(&pending).unwrap();
        transaction.execute("INSERT INTO process_inbox(process_key,input_id,input,checksum) VALUES(?1,?2,?3,?4)", params![key, pending.input_id.as_str(), bytes, ContentDigest::sha256(&bytes).0.as_slice()]).unwrap();
        transaction
            .execute(
                "UPDATE process_usage SET bytes_used=bytes_used+?2 WHERE process_key=?1",
                params![key, i64::try_from(bytes.len()).unwrap() + 128],
            )
            .unwrap();
    }
    transaction
        .execute_batch("DROP INDEX process_inbox_pending; PRAGMA user_version=5;")
        .unwrap();
    transaction.commit().unwrap();
}
#[test]
fn sigkill_during_populated_v5_inbox_upgrade_preserves_all_process_rows() {
    for phase in [
        "before-transaction",
        "after-begin",
        "after-v5",
        "after-inbox-index",
        "after-v6",
        "before-commit",
        "after-commit",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("populated.db");
        seed_v5(&path);
        let original = rows(&Connection::open(&path).unwrap());
        let marker = directory.path().join("marker");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "schema::tests::migration_process_helper",
                "--nocapture",
            ])
            .env("PENELOPE_MIGRATION_DB", &path)
            .env("PENELOPE_MIGRATION_MARKER", &marker)
            .env("PENELOPE_MIGRATION_CUT", phase)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read(&marker).ok().as_deref() != Some(phase.as_bytes()) {
            if Instant::now() >= deadline {
                drop(child.kill());
                drop(child.wait());
                panic!("populated migration failed to reach {phase}");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        child.kill().unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        let connection = Connection::open(&path).unwrap();
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            version,
            if phase == "after-commit" { 6 } else { 5 },
            "{phase}"
        );
        let indexed: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='process_inbox_pending')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed, phase == "after-commit", "{phase}");
        assert_eq!(rows(&connection), original, "{phase}");
        drop(connection);
        let reopened = open(&path, SqliteLimits::default()).unwrap();
        assert_eq!(rows(&reopened), original, "{phase}");
        assert_eq!(
            reopened
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            6
        );
        println!("populated-v5 phase={phase} signal=9 rows_preserved=true reopen_version=6");
    }
}
