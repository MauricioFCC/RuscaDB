//! Aceptación de SPEC-0189 (max_result_rows).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

#[test] // @spec AC-0189-01
fn test_ac_0189_01_max_result_rows() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("limits.db");
    let mut config = DbConfig::new(&path, 64);
    config.max_result_rows = Some(2);
    let mut database = Database::open(config).expect("open");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "x".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create");
    for value in 1..=5 {
        database
            .insert(
                "t",
                ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
            )
            .expect("insert");
    }
    let rows = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(rows.len(), 2);
}
