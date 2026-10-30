//! Cableado de adapters en la fachada (SPEC-0173/0174/0175/0176/0177).
//!
//! Cubre las rutas publicas que conectan los modulos nuevos con el motor:
//! estimacion de cardinalidad con indice aprendido y snapshot del indice
//! vectorial.

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Abre una base temporal de pruebas.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
    (dir, database)
}

/// El planificador estima el rango con el indice aprendido (SPEC-0174).
#[test]
fn estimate_distinct_range_uses_learned_index() {
    let (_dir, mut database) = open_test_db("learned_est");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
    for value in 0..200i64 {
        let scalars = ScalarMap::from([("a".to_string(), ScalarValue::Int(value * 3))]);
        database.insert("t", scalars).expect("insert");
    }

    assert_eq!(
        database
            .estimate_distinct_range("t", "a", 0, 0)
            .expect("rango"),
        1
    );
    assert_eq!(
        database
            .estimate_distinct_range("t", "a", 3, 9)
            .expect("rango"),
        3
    );
    assert_eq!(
        database
            .estimate_distinct_range("t", "a", 10, 11)
            .expect("rango"),
        0
    );
    assert_eq!(
        database
            .estimate_distinct_range("t", "a", -5, 1000)
            .expect("rango"),
        200
    );
}
