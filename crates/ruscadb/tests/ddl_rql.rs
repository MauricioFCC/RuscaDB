//! SPEC-0209 — `DROP INDEX` y `DROP TABLE` en RQL.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};
use ruscadb_query::{Statement, parse_statement};

fn open(dir: &std::path::Path) -> Database {
    let path = dir.join("ddl-rql.db");
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "k".to_string(),
            col_type: ColumnType::Int,
        }],
    )
    .expect("create");
    db.insert(
        "t",
        ScalarMap::from([("k".to_string(), ScalarValue::Int(1))]),
    )
    .expect("insert");
    db.create_index("t", "k").expect("index");
    db
}

/// AC-0209-01 — `DROP INDEX t.k` elimina el índice y el planner cae a FullScan.
#[test] // @spec AC-0209-01
fn test_ac_0209_01_drop_index_rql() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    assert!(db.execute("DROP INDEX t.k").expect("drop index").is_empty());
    let plan = db
        .explain("EXPLAIN SELECT * FROM t WHERE k = 1")
        .expect("explain")
        .join("\n");
    assert!(plan.contains("FullScan(t)"), "{plan}");
    assert!(
        db.execute("DROP INDEX t.k")
            .expect("idempotente")
            .is_empty(),
        "segundo drop no falla"
    );
}

/// AC-0209-02 — `DROP TABLE t` elimina la tabla; el AST/Display es estable.
#[test] // @spec AC-0209-02
fn test_ac_0209_02_drop_table_rql() {
    let statement = parse_statement("DROP TABLE t").expect("parse");
    assert!(matches!(statement, Statement::DropTable { ref table } if table == "t"));
    assert_eq!(statement.to_string(), "DROP TABLE t");

    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    assert!(db.execute("DROP TABLE t").expect("drop table").is_empty());
    assert!(db.execute("SELECT * FROM t").is_err());
    assert!(db.execute("DROP TABLE t").expect("idempotente").is_empty());
}
