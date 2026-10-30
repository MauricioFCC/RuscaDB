//! Vistas materializadas cableadas al executor (SPEC-0149).

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

/// Crea `t(a INT)` y siembra tres filas.
fn seed(database: &mut Database) {
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
    for value in 1..=3 {
        database
            .insert(
                "t",
                ScalarMap::from([("a".to_string(), ScalarValue::Int(value))]),
            )
            .expect("insert");
    }
}

/// Crear, refrescar, listar y borrar una vista materializada.
#[test]
fn materialized_view_lifecycle() {
    let (_dir, mut database) = open_test_db("mv");
    seed(&mut database);

    let count = database
        .create_materialized_view("v", "SELECT a FROM t")
        .expect("create view");
    assert_eq!(count, 3);
    assert_eq!(
        database
            .materialized_view_rows("v")
            .expect("rows")
            .iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>(),
        vec!["1", "2", "3"]
    );
    assert_eq!(database.materialized_view_names(), vec!["v".to_string()]);

    // La vista no ve escrituras hasta el refresh.
    database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(4))]),
        )
        .expect("insert");
    assert_eq!(database.materialized_view_rows("v").expect("rows").len(), 3);
    assert_eq!(database.refresh_materialized_view("v").expect("refresh"), 4);
    assert_eq!(database.materialized_view_rows("v").expect("rows").len(), 4);

    assert!(database.drop_materialized_view("v").expect("drop"));
    assert!(!database.drop_materialized_view("v").expect("drop"));
    assert!(database.materialized_view_rows("v").is_none());
    assert!(database.refresh_materialized_view("v").is_err());
}

/// Errores accionables: vista inexistente y consulta invalida.
#[test]
fn materialized_view_errors_are_actionable() {
    let (_dir, mut database) = open_test_db("mv_err");
    seed(&mut database);
    let error = database
        .refresh_materialized_view("missing")
        .expect_err("vista inexistente");
    assert!(error.to_string().contains("missing"), "{error}");
    assert!(
        database
            .create_materialized_view("bad", "SELECT * FROM no_existe")
            .is_err()
    );
    assert!(database.materialized_view_names().is_empty());
}

/// Las vistas sobreviven a cerrar y reabrir la base (sidecar JSON).
#[test]
fn materialized_view_is_persistent() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("mv_persist.db");
    {
        let mut database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
        seed(&mut database);
        database
            .create_materialized_view("v", "SELECT a FROM t")
            .expect("create view");
    }
    let database = Database::open(DbConfig::new(&path, 128)).expect("reapertura");
    assert_eq!(
        database
            .materialized_view_rows("v")
            .expect("vista persistida")
            .len(),
        3
    );
    assert_eq!(database.materialized_view_names(), vec!["v".to_string()]);
}

/// El refresh devuelve el delta por fingerprints (sin recalcular filas).
#[test]
fn refresh_delta_reports_inserted_and_deleted() {
    let (_dir, mut database) = open_test_db("mv_delta");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
    let mut ids = Vec::new();
    for value in 1..=3 {
        ids.push(
            database
                .insert(
                    "t",
                    ScalarMap::from([("a".to_string(), ScalarValue::Int(value))]),
                )
                .expect("insert"),
        );
    }
    database
        .create_materialized_view("v", "SELECT a FROM t")
        .expect("view");

    // Sin cambios: no-op.
    let delta = database
        .refresh_materialized_view_delta("v")
        .expect("delta");
    assert!(delta.is_noop(), "{delta:?}");
    assert_eq!(delta.unchanged, 3);

    // Insercion: 1 nueva, 3 iguales.
    database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(4))]),
        )
        .expect("insert");
    let delta = database
        .refresh_materialized_view_delta("v")
        .expect("delta");
    assert_eq!((delta.inserted, delta.deleted, delta.unchanged), (1, 0, 3));
    assert_eq!(delta.changed(), 1);

    // Borrado: 1 eliminada, 3 iguales.
    database.delete("t", &ids[0]).expect("delete");
    let delta = database
        .refresh_materialized_view_delta("v")
        .expect("delta");
    assert_eq!((delta.inserted, delta.deleted, delta.unchanged), (0, 1, 3));
}
