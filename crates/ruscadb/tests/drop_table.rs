//! SPEC-0207 — DROP TABLE (API) y limpieza de metadatos en memoria.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Crea `t(k Int)` con 3 filas e índice.
fn open(dir: &std::path::Path) -> Database {
    let path = dir.join("drop-table.db");
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "k".to_string(),
            col_type: ColumnType::Int,
        }],
    )
    .expect("create");
    for value in 0..3 {
        db.insert(
            "t",
            ScalarMap::from([("k".to_string(), ScalarValue::Int(value))]),
        )
        .expect("insert");
    }
    db.create_index("t", "k").expect("index");
    db
}

/// AC-0207-01 — drop_table elimina la tabla, las consultas fallan y repetir es
/// no-op.
#[test] // @spec AC-0207-01
fn test_ac_0207_01_drop_table_removes_table() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    assert!(db.drop_table("t").expect("drop"), "existía");
    assert!(db.execute("SELECT * FROM t").is_err(), "tabla eliminada");
    assert!(!db.drop_table("t").expect("drop again"), "ya no existe");
    assert!(db.stats("t").is_none(), "stats eliminadas");
}

/// AC-0207-02 — recrear la tabla empieza limpio (sin índice ni filas viejas).
#[test] // @spec AC-0207-02
fn test_ac_0207_02_recreate_table_is_clean() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    db.drop_table("t").expect("drop");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "k".to_string(),
            col_type: ColumnType::Int,
        }],
    )
    .expect("recreate");
    db.insert(
        "t",
        ScalarMap::from([("k".to_string(), ScalarValue::Int(42))]),
    )
    .expect("insert");
    let rows = db.execute("SELECT * FROM t").expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("k"), Some(&ScalarValue::Int(42)));
    let plan = db
        .explain("EXPLAIN SELECT * FROM t WHERE k = 42")
        .expect("explain")
        .join("\n");
    assert!(plan.contains("FullScan(t)"), "{plan}");
}
