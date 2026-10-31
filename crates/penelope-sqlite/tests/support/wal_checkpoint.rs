//! A retained reader snapshot must prevent a false truncation acknowledgement.
use super::*;
#[test]
fn wal_checkpoint_reports_busy_then_truncates_without_changing_task_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("checkpoint-reader.db");
    let mut journal = SqliteTaskJournal::open(
        &path,
        definition(),
        SqliteLimits {
            busy_timeout: std::time::Duration::ZERO,
            ..SqliteLimits::default()
        },
    )
    .unwrap();
    let projection = committed_start(&mut journal);
    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN DEFERRED").unwrap();
    let old_count: u32 = reader
        .query_row("SELECT count(*) FROM task_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(old_count, 2);
    let output = BlobRef {
        digest: Digest::of(b"done"),
        bytes: 4,
        schema: 1,
    };
    let complete = projection
        .propose(
            TaskExecutionEvent::Completed { output },
            projection.generation(),
            TaskTick::new(2),
        )
        .unwrap();
    journal.commit(&complete, Some(b"done")).unwrap();
    let observer = Connection::open(&path).unwrap();
    let tables = [
        "task_records",
        "task_heads",
        "task_blobs",
        "task_retirements",
        "runtime_generations",
    ];
    let before = write_snapshot::snapshot(&observer, &tables);
    let busy = journal.checkpoint_wal().unwrap();
    let wal_path = path.with_extension("db-wal");
    assert!(std::fs::metadata(&wal_path).unwrap().len() > 0);
    assert_eq!(busy.busy, 1);
    assert!(busy.wal_frames > 0);
    assert!(busy.checkpointed_frames >= 0);
    assert!(busy.checkpointed_frames < busy.wal_frames);
    assert_eq!(write_snapshot::snapshot(&observer, &tables), before);
    // Checkpointing cannot advance the reader's transaction snapshot.
    let pinned_count: u32 = reader
        .query_row("SELECT count(*) FROM task_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(pinned_count, old_count);
    assert_eq!(journal.read(scope(), complete.sequence), Ok(Some(complete)));
    reader.execute_batch("ROLLBACK").unwrap();
    let truncated = journal.checkpoint_wal().unwrap();
    assert_eq!(truncated.busy, 0);
    assert_eq!(truncated.wal_frames, 0);
    assert_eq!(truncated.checkpointed_frames, 0);
    assert_eq!(std::fs::metadata(&wal_path).unwrap().len(), 0);
    assert_eq!(write_snapshot::snapshot(&observer, &tables), before);
    let current_count: u32 = reader
        .query_row("SELECT count(*) FROM task_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(current_count, 3);
    let mut buffer = [0xa5; 8];
    assert_eq!(journal.read_blob(scope(), output, &mut buffer), Ok(4));
    assert_eq!(&buffer[..4], b"done");
    assert_eq!(&buffer[4..], &[0xa5; 4]);
    drop(journal);
    let mut reopened = open(&path);
    assert_eq!(
        reopened.read(scope(), TaskSequence::ZERO),
        Ok(Some(intent()))
    );
    assert_eq!(
        reopened.read(scope(), complete.sequence),
        Ok(Some(complete))
    );
    reopened.rebuild_head(scope()).unwrap();
    assert_eq!(write_snapshot::snapshot(&observer, &tables), before);
    reopened.check_integrity().unwrap();
    println!(
        "busy=true pinned_records=2 released_records=3 truncated=true history_unchanged=true reopened=true"
    );
}
