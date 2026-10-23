//! Tests de aceptación de apertura solo lectura (SPEC-0109).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Abre una base temporal de pruebas.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 64)).expect("apertura write");
    (dir, database)
}

/// Crea la tabla `t(id INT, name TEXT)` y siembra una fila.
fn seed(database: &mut Database) {
    database
        .create_table(
            "t",
            vec![
                ColumnDef { name: "id".to_string(), col_type: ColumnType::Int },
                ColumnDef { name: "name".to_string(), col_type: ColumnType::Text },
            ],
        )
        .expect("create_table");
    database
        .insert(
            "t",
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(1)),
                ("name".to_string(), ScalarValue::Text("uno".to_string())),
            ]),
        )
        .expect("insert");
}

#[test] // @spec AC-0109-01
fn test_ac_0109_01_readonly_open_succeeds() {
    let (dir, mut database) = open_test_db("ac0109_01");
    seed(&mut database);
    let path = dir.path().join("ac0109_01.db");
    let mut reader = Database::open_readonly(DbConfig::new(&path, 64)).expect("open readonly");
    assert!(reader.is_read_only());
    let rows = reader.execute("SELECT id, name FROM t").expect("select");
    assert_eq!(rows.len(), 1);
}

#[test] // @spec AC-0109-02
fn test_ac_0109_02_readonly_rejects_writes() {
    let (dir, mut database) = open_test_db("ac0109_02");
    seed(&mut database);
    let path = dir.path().join("ac0109_02.db");
    let mut reader = Database::open_readonly(DbConfig::new(&path, 64)).expect("open readonly");
    let error = reader
        .insert(
            "t",
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(2)),
                ("name".to_string(), ScalarValue::Text("dos".to_string())),
            ]),
        )
        .expect_err("insert debe fallar");
    assert!(error.to_string().contains("solo lectura"), "error: {error}");
    let error = reader
        .execute("INSERT INTO t (id, name) VALUES (3, 'tres')")
        .expect_err("execute DML debe fallar");
    assert!(error.to_string().contains("solo lectura"), "error: {error}");
}
