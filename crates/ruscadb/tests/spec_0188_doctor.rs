//! Aceptacion de SPEC-0188 (doctor y metricas).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

fn open(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(format!("{tag}.db"));
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "x".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create");
    for value in 1..=2 {
        database
            .insert(
                "t",
                ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
            )
            .expect("insert");
    }
    (dir, database)
}

#[test] // @spec AC-0188-01
fn test_ac_0188_01_doctor() {
    let (_dir, mut database) = open("doctor");
    let report = database.doctor().expect("doctor");
    assert!(report.schema_version >= 1);
    assert!(report.integrity_ok);
    assert!(report.wal_ok);
    assert!(report.heap_ok);
    assert!(report.vector_ok);
    assert!(report.last_tx_id > 0);
    assert!(report.features.contains(&"kms"));
}

#[test] // @spec AC-0188-02
fn test_ac_0188_02_metrics() {
    let (_dir, mut database) = open("metrics");
    let before = database.metrics();
    let rows = database.execute("SELECT * FROM t").expect("select");
    let after = database.metrics();
    assert!(after.queries > before.queries);
    assert!(after.rows_returned >= rows.len() as u64);
    assert!(after.queries >= 1);
}
