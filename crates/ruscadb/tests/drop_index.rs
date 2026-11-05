//! SPEC-0205 — eliminar un índice secundario y volver a planificar.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Crea `t(k Int)` con 10 filas.
fn open(dir: &std::path::Path) -> Database {
    let path = dir.join("drop-index.db");
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "k".to_string(),
            col_type: ColumnType::Int,
        }],
    )
    .expect("create");
    for value in 0..10 {
        db.insert(
            "t",
            ScalarMap::from([("k".to_string(), ScalarValue::Int(value))]),
        )
        .expect("insert");
    }
    db
}

/// AC-0205-01 — drop_index elimina el índice, el planner cae a FullScan y un
/// segundo drop es no-op.
#[test] // @spec AC-0205-01
fn test_ac_0205_01_drop_index_falls_back() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    db.create_index("t", "k").expect("create index");
    let indexed = db
        .explain("EXPLAIN SELECT * FROM t WHERE k = 3")
        .expect("explain")
        .join("\n");
    assert!(indexed.contains("IndexScan(t.k)"), "{indexed}");

    assert!(db.drop_index("t", "k").expect("drop"), "existía");
    let scan = db
        .explain("EXPLAIN SELECT * FROM t WHERE k = 3")
        .expect("explain")
        .join("\n");
    assert!(scan.contains("FullScan(t)"), "{scan}");
    assert!(
        !db.drop_index("t", "k").expect("drop again"),
        "ya no existe"
    );
}

/// AC-0205-02 — tras recrear el índice los datos siguen intactos y el planner
/// vuelve a usarlo (la caché de planes se invalida por la época).
#[test] // @spec AC-0205-02
fn test_ac_0205_02_recreate_index() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    db.create_index("t", "k").expect("create");
    db.execute("SELECT * FROM t WHERE k = 3").expect("cache");
    db.drop_index("t", "k").expect("drop");
    db.create_index("t", "k").expect("recreate");
    let indexed = db
        .explain("EXPLAIN SELECT * FROM t WHERE k = 3")
        .expect("explain")
        .join("\n");
    assert!(indexed.contains("IndexScan(t.k)"), "{indexed}");
    let rows = db.execute("SELECT * FROM t WHERE k = 3").expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("k"), Some(&ScalarValue::Int(3)));
}
