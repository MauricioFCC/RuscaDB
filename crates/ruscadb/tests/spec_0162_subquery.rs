//! Aceptación de subconsultas `IN (SELECT ...)` no correlacionadas (SPEC-0162).

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
    for value in [1i64, 2, 3] {
        database
            .insert(
                "a",
                ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
            )
            .expect("a");
    }
    for value in [2i64, 3] {
        database
            .insert(
                "b",
                ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
            )
            .expect("b");
    }
    (dir, database)
}

/// `IN (SELECT ...)` materializa la subconsulta una vez.
#[test] // @spec AC-0162-02
fn test_ac_0162_02_in_subquery_sql() {
    let (_dir, mut database) = seed("in_subquery");
    let rows = database
        .execute("SELECT x FROM a WHERE x IN (SELECT x FROM b)")
        .expect("in subquery");
    assert_eq!(rows.len(), 2);
    let rows = database
        .execute("SELECT x FROM a WHERE x NOT IN (SELECT x FROM b)")
        .expect("not in subquery");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("x"), Some(&ScalarValue::Int(1)));
}

/// Subconsulta escalar no correlacionada y error si devuelve >1 fila.
#[test] // @spec AC-0162-01
fn test_ac_0162_01_scalar_subquery_sql() {
    let (_dir, mut database) = seed("scalar_subquery");
    let rows = database
        .execute("SELECT x FROM a WHERE x = (SELECT MAX(x) FROM b)")
        .expect("scalar subquery");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("x"), Some(&ScalarValue::Int(3)));

    let empty = database
        .execute("SELECT x FROM a WHERE x = (SELECT x FROM b WHERE x > 99)")
        .expect("scalar vacia");
    assert_eq!(empty.len(), 0, "NULL escalar no es TRUE");

    let error = database
        .execute("SELECT x FROM a WHERE x = (SELECT x FROM b)")
        .expect_err(">1 fila");
    assert_eq!(error.code(), "TYPE_MISMATCH");
}

/// `EXISTS`/`NOT EXISTS` no correlacionados.
#[test] // @spec AC-0162-03
fn test_ac_0162_03_exists_sql() {
    let (_dir, mut database) = seed("exists");
    let rows = database
        .execute("SELECT x FROM a WHERE EXISTS (SELECT x FROM b WHERE x = 3)")
        .expect("exists");
    assert_eq!(rows.len(), 3);
    let rows = database
        .execute("SELECT x FROM a WHERE EXISTS (SELECT x FROM b WHERE x = 99)")
        .expect("exists vacio");
    assert_eq!(rows.len(), 0);
    let rows = database
        .execute("SELECT x FROM a WHERE NOT EXISTS (SELECT x FROM b WHERE x = 99)")
        .expect("not exists");
    assert_eq!(rows.len(), 3);
}
