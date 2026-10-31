//! A warm validated projection must not conceal a changed durable byte counter.
use super::*;

fn damaged_usage(rebuilt: bool, increase: bool) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cached-usage.db");
    let mut store = open(&path);
    let first = intent();
    store.commit(&first, Some(b"input")).unwrap();
    if rebuilt {
        store.rebuild_head(scope()).unwrap();
    }
    let mut projection = TaskProjection::new(scope(), definition()).unwrap();
    projection.apply(&first).unwrap();
    let next = projection
        .propose(
            TaskExecutionEvent::Started,
            TaskGeneration::new(1),
            TaskTick::new(1),
        )
        .unwrap();
    let connection = Connection::open(&path).unwrap();
    let original_head: (u32, Vec<u8>, i64) = connection
        .query_row(
            "SELECT next_sequence,last_digest,bytes_used FROM task_heads",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let original = original_head.2;
    assert!(original > 0);
    let changed = if increase {
        original.checked_add(1).unwrap()
    } else {
        0
    };
    connection
        .execute("UPDATE task_heads SET bytes_used=?1", [changed])
        .unwrap();
    assert_eq!(store.commit(&next, None), Err(TaskPortError::Corrupt));
    let head: (u32, Vec<u8>, i64) = connection
        .query_row(
            "SELECT next_sequence,last_digest,bytes_used FROM task_heads",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(head, (original_head.0, original_head.1, changed));
    let records: u32 = connection
        .query_row("SELECT count(*) FROM task_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(records, 1);
    assert_eq!(store.read(scope(), TaskSequence::new(1)).unwrap(), None);
    // Explicit maintenance repair revalidates immutable records and blobs and
    // restores the derived byte count, then the exact append succeeds.
    store.rebuild_head(scope()).unwrap();
    let repaired: i64 = connection
        .query_row("SELECT bytes_used FROM task_heads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(repaired, original);
    assert_eq!(store.commit(&next, None).unwrap(), next);
    drop(store);
    let mut reopened = open(&path);
    assert_eq!(
        reopened.read(scope(), TaskSequence::new(1)).unwrap(),
        Some(next)
    );
    let TaskExecutionEvent::IntentCommitted {
        input: reference, ..
    } = first.event
    else {
        panic!("expected input intent");
    };
    let mut bytes = [0; 5];
    assert_eq!(
        reopened.read_blob(scope(), reference, &mut bytes).unwrap(),
        5
    );
    assert_eq!(&bytes, b"input");
}
#[test]
fn committed_cache_rejects_lower_usage_until_repaired() {
    damaged_usage(false, false);
}
#[test]
fn committed_cache_rejects_higher_usage_until_repaired() {
    damaged_usage(false, true);
}
#[test]
fn rebuilt_cache_rejects_lower_usage_until_repaired() {
    damaged_usage(true, false);
}
#[test]
fn rebuilt_cache_rejects_higher_usage_until_repaired() {
    damaged_usage(true, true);
}
