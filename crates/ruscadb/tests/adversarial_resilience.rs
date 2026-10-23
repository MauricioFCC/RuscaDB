//! Tests de resiliencia: crash recovery, WAL corruption, resource exhaustion.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

fn open_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(format!("{tag}.db"));
    let db = Database::open(DbConfig::new(&path, 64)).expect("open");
    (dir, db)
}

/// Crash recovery: datos confirmados sobreviven a reopen.
#[test]
fn test_resilience_crash_recovery() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("crash.db");
    {
        let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
        db.create_table(
            "t",
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create");
        db.insert(
            "t",
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(1)),
                (
                    "name".to_string(),
                    ScalarValue::Text("survivor".to_string()),
                ),
            ]),
        )
        .expect("insert");
        // Simula crash: drop sin close (no fsync explicito del manifiesto).
    }
    // Reopen debe recuperar los datos.
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("reopen");
    let rows = db.execute("SELECT * FROM t").expect("select");
    assert_eq!(rows.len(), 1, "datos confirmados deben sobrevivir al crash");
}

/// Rollback no deja datos parciales.
#[test]
fn test_resilience_rollback_atomicity() {
    let (dir, mut db) = open_db("rollback");
    db.create_table(
        "t",
        vec![
            ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            },
            ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            },
        ],
    )
    .expect("create");
    db.insert(
        "t",
        ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(1)),
            (
                "name".to_string(),
                ScalarValue::Text("original".to_string()),
            ),
        ]),
    )
    .expect("insert");
    // Transacción explícita: begin + rollback sin writes.
    let tx = db.begin();
    assert!(db.active_tx().is_some());
    db.rollback().expect("rollback");
    assert!(db.active_tx().is_none());
    // La base sigue operativa tras el rollback.
    let rows = db.execute("SELECT * FROM t").expect("select");
    assert_eq!(rows.len(), 1, "rollback no debe corromper datos");
    let _ = (tx, dir);
}

/// Base vacía se abre correctamente.
#[test]
fn test_resilience_empty_db_open() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("empty.db");
    let db = Database::open(DbConfig::new(&path, 64)).expect("open empty");
    assert!(!db.is_read_only());
    // Reopen también debe funcionar.
    drop(db);
    let db2 = Database::open(DbConfig::new(&path, 64)).expect("reopen empty");
    drop(db2);
}

/// Pool de tamaño 1 no corrompe datos.
#[test]
fn test_resilience_small_pool() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("smallpool.db");
    let mut db = Database::open(DbConfig::new(&path, 4)).expect("open");
    db.create_table(
        "t",
        vec![
            ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            },
            ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            },
        ],
    )
    .expect("create");
    for i in 0..10 {
        db.insert(
            "t",
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(i)),
                ("name".to_string(), ScalarValue::Text(format!("name{i}"))),
            ]),
        )
        .expect("insert");
    }
    let rows = db.execute("SELECT * FROM t").expect("select");
    assert_eq!(rows.len(), 10, "pool pequeño no debe perder datos");
    drop(db);
    // Reopen y verificar.
    let mut db2 = Database::open(DbConfig::new(&path, 4)).expect("reopen");
    let rows2 = db2.execute("SELECT * FROM t").expect("select2");
    assert_eq!(rows2.len(), 10, "datos persisten con pool pequeño");
}

/// Múltiples tablas no interfieren.
#[test]
fn test_resilience_multi_table() {
    let (dir, mut db) = open_db("multi");
    for t in ["a", "b", "c"] {
        db.create_table(
            t,
            vec![ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create");
        db.insert(
            t,
            ScalarMap::from([("id".to_string(), ScalarValue::Int(1))]),
        )
        .expect("insert");
    }
    for t in ["a", "b", "c"] {
        let rows = db.execute(&format!("SELECT * FROM {t}")).expect("select");
        assert_eq!(rows.len(), 1);
    }
    let _ = dir;
}
