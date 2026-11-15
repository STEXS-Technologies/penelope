//! Reproduce the released-to-this-branch v8 paired consumer/receipt constraint.
pub fn downgrade_to_v8(sql: &rusqlite::Connection) {
    let schema: String = sql
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='machine_intent_delivery'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let old = schema.replace(
        "CHECK(receipt IS NULL OR consumer IS NOT NULL)",
        "CHECK((consumer IS NULL)=(receipt IS NULL))",
    );
    assert_ne!(old, schema);
    sql.execute_batch("DROP INDEX machine_delivery_pending; ALTER TABLE machine_intent_delivery RENAME TO newer_delivery;").unwrap();
    sql.execute_batch(&old).unwrap();
    sql.execute_batch("INSERT INTO machine_intent_delivery SELECT machine_key,intent,owner,token,expires,attempt,CASE WHEN receipt IS NULL THEN NULL ELSE consumer END,receipt FROM newer_delivery; DROP TABLE newer_delivery; CREATE INDEX machine_delivery_pending ON machine_intent_delivery(machine_key,expires,intent) WHERE receipt IS NULL; PRAGMA user_version=8;").unwrap();
}
