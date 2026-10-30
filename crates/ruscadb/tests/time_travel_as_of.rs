//! Time-travel por `tx_id`: replay `--as-of` (SPEC-0129/0158).

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Abre una base temporal.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
    (dir, database)
}

/// `execute_at_lsn` ve el estado historico y no el actual.
#[test]
fn execute_at_lsn_sees_historical_state() {
    let (_dir, mut database) = open_test_db("as_of");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
    database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(1))]),
        )
        .expect("insert 1");
    let as_of = database.snapshot().tx_id;

    database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(2))]),
        )
        .expect("insert 2");

    // Estado actual: dos filas.
    assert_eq!(
        database.execute("SELECT a FROM t").expect("select").len(),
        2
    );
    // Estado historico: solo la primera.
    let historical = database
        .execute_at_lsn("SELECT a FROM t", as_of)
        .expect("as-of");
    assert_eq!(historical.len(), 1, "solo la fila del snapshot {as_of}");
    assert_eq!(historical[0].get("a"), Some(&ScalarValue::Int(1)));

    // `0` equivale al snapshot actual.
    assert_eq!(
        database
            .execute_at_lsn("SELECT a FROM t", 0)
            .expect("actual")
            .len(),
        2
    );
    // Snapshot construible desde la API publica.
    let snapshot = ruscadb::Snapshot::at(as_of);
    assert_eq!(
        database
            .execute_at("SELECT a FROM t", &snapshot)
            .expect("snapshot")
            .len(),
        1
    );
}

/// Un borrado posterior no es visible en el snapshot historico.
#[test]
fn historical_snapshot_ignores_later_delete() {
    let (_dir, mut database) = open_test_db("as_of_delete");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
    let id = database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(7))]),
        )
        .expect("insert");
    let as_of = database.snapshot().tx_id;
    database.delete("t", &id).expect("delete");

    assert!(
        database
            .execute("SELECT a FROM t")
            .expect("actual")
            .is_empty()
    );
    assert_eq!(
        database
            .execute_at_lsn("SELECT a FROM t", as_of)
            .expect("as-of")
            .len(),
        1,
        "la fila borrada sigue visible en el pasado"
    );
}
