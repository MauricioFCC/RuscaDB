//! SPEC-0206 — `ANALYZE <tabla>` en RQL.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};
use ruscadb_query::{Statement, parse_statement};

/// Crea `t(k Int)` con 4 filas.
fn open(dir: &std::path::Path) -> Database {
    let path = dir.join("analyze-rql.db");
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "k".to_string(),
            col_type: ColumnType::Int,
        }],
    )
    .expect("create");
    for value in 1..=4 {
        db.insert(
            "t",
            ScalarMap::from([("k".to_string(), ScalarValue::Int(value))]),
        )
        .expect("insert");
    }
    db
}

/// AC-0206-01 — `ANALYZE t` refresca stats y devuelve 0 filas.
#[test] // @spec AC-0206-01
fn test_ac_0206_01_analyze_rql_refreshes_stats() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    let rows = db.execute("ANALYZE t").expect("analyze");
    assert!(rows.is_empty());
    let stats = db.stats("t").expect("stats frescas");
    assert_eq!(stats.rows, 4);
}

/// AC-0206-02 — el parser produce `Statement::Analyze`, el Display es estable y
/// una tabla inexistente falla.
#[test] // @spec AC-0206-02
fn test_ac_0206_02_analyze_parser_and_errors() {
    let statement = parse_statement("ANALYZE t").expect("parse");
    assert!(matches!(statement, Statement::Analyze { ref table } if table == "t"));
    assert_eq!(statement.to_string(), "ANALYZE t");

    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    assert!(db.execute("ANALYZE missing").is_err());
}
