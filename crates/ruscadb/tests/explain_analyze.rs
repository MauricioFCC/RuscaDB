//! SPEC-0204 — `EXPLAIN ANALYZE` (filas y tiempo reales).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Crea `t(k Int)` con 3 filas.
fn open(dir: &std::path::Path) -> Database {
    let path = dir.join("explain-analyze.db");
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "k".to_string(),
            col_type: ColumnType::Int,
        }],
    )
    .expect("create");
    for value in 1..=3 {
        db.insert(
            "t",
            ScalarMap::from([("k".to_string(), ScalarValue::Int(value))]),
        )
        .expect("insert");
    }
    db
}

/// AC-0204-01 — el plan incluye `ACTUAL_ROWS`/`ACTUAL_TIME_MS` y coincide con
/// la ejecución real.
#[test] // @spec AC-0204-01
fn test_ac_0204_01_explain_analyze_reports_actuals() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    let lines = db
        .explain_analyze("EXPLAIN SELECT * FROM t WHERE k >= 2")
        .expect("explain analyze");
    let text = lines.join("\n");
    assert!(text.contains("ACTUAL_ROWS 2"), "{text}");
    assert!(text.contains("ACTUAL_TIME_MS"), "{text}");
    let actual = db.execute("SELECT * FROM t WHERE k >= 2").expect("execute");
    assert_eq!(actual.len(), 2);
    let actual_line = lines
        .iter()
        .find(|line| line.starts_with("ACTUAL_ROWS"))
        .expect("line");
    assert_eq!(actual_line, "ACTUAL_ROWS 2");
}

/// AC-0204-02 — solo acepta `EXPLAIN SELECT`; una sentencia normal o DML falla.
#[test] // @spec AC-0204-02
fn test_ac_0204_02_explain_analyze_rejects_non_select() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    assert!(
        db.explain_analyze("SELECT * FROM t").is_err(),
        "sin EXPLAIN debe fallar"
    );
    assert!(
        db.explain_analyze("EXPLAIN DELETE FROM t").is_err(),
        "EXPLAIN de DML debe fallar"
    );
}
