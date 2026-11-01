//! Aceptación SQL de operaciones de conjuntos (SPEC-0164).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Base con `a(x INT)` y `b(x INT)`.
fn seed(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(format!("{tag}.db"));
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    for table in ["a", "b"] {
        database
            .create_table(
                table,
                vec![ColumnDef {
                    name: "x".to_string(),
                    col_type: ColumnType::Int,
                }],
            )
            .expect("create");
    }
    for value in [1i64, 2, 2] {
        database
            .insert(
                "a",
                ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
            )
            .expect("insert a");
    }
    for value in [2i64, 3] {
        database
            .insert(
                "b",
                ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
            )
            .expect("insert b");
    }
    (dir, database)
}

/// AC-0164-01: `UNION` deduplica en SQL.
#[test] // @spec AC-0164-01
fn test_ac_0164_01_union_sql() {
    let (_dir, mut database) = seed("union");
    let rows = database
        .execute("SELECT x FROM a UNION SELECT x FROM b")
        .expect("union");
    assert_eq!(rows.len(), 3);
}

/// AC-0164-02: `UNION ALL` conserva duplicados.
#[test] // @spec AC-0164-02
fn test_ac_0164_02_union_all_sql() {
    let (_dir, mut database) = seed("union_all");
    let rows = database
        .execute("SELECT x FROM a UNION ALL SELECT x FROM b")
        .expect("union all");
    assert_eq!(rows.len(), 5);
}

/// AC-0164-03: `INTERSECT` conserva solo las filas comunes.
#[test] // @spec AC-0164-03
fn test_ac_0164_03_intersect_sql() {
    let (_dir, mut database) = seed("intersect");
    let rows = database
        .execute("SELECT x FROM a INTERSECT SELECT x FROM b")
        .expect("intersect");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("x"), Some(&ScalarValue::Int(2)));
}

/// AC-0164-04: `EXCEPT` resta el lado derecho.
#[test] // @spec AC-0164-04
fn test_ac_0164_04_except_sql() {
    let (_dir, mut database) = seed("except");
    let rows = database
        .execute("SELECT x FROM a EXCEPT SELECT x FROM b")
        .expect("except");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("x"), Some(&ScalarValue::Int(1)));
}
