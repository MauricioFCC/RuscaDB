//! Aceptación de SPEC-0183 (3VL/DISTINCT) y SPEC-0184 (prepared/códigos).

#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, ParamType, ParamValue, RuscaError, ScalarMap,
    ScalarValue,
};

/// Abre una base temporal con `t(a INT, b INT)`.
fn open_nullable(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio");
    let path = dir.path().join(format!("{tag}.db"));
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "t",
            vec![
                ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "b".to_string(),
                    col_type: ColumnType::Int,
                },
            ],
        )
        .expect("create_table");
    for (a, b) in [
        (ScalarValue::Int(1), ScalarValue::Null),
        (ScalarValue::Int(2), ScalarValue::Int(2)),
        (ScalarValue::Null, ScalarValue::Null),
    ] {
        database
            .insert(
                "t",
                ScalarMap::from([("a".to_string(), a), ("b".to_string(), b)]),
            )
            .expect("insert");
    }
    (dir, database)
}

/// AC-0183-01: precedencia `NOT > AND > OR` y cortocircuito.
#[test] // @spec AC-0183-01
fn test_ac_0183_01_or_not_precedence() {
    let (_dir, mut database) = open_nullable("or_not");
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a = 1 OR b = 2")
            .expect("or")
            .len(),
        2
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE NOT (a = 1)")
            .expect("not")
            .len(),
        1,
        "NOT de UNKNOWN sigue siendo UNKNOWN"
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a = 1 OR b = 2 AND a = 2")
            .expect("precedencia")
            .len(),
        2,
        "AND se evalúa antes que OR"
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE NOT NOT a = 1")
            .expect("doble not")
            .len(),
        1
    );
}

/// AC-0183-02: `NULL` produce UNKNOWN; `IS [NOT] NULL` no.
#[test] // @spec AC-0183-02
fn test_ac_0183_02_null_semantics() {
    let (_dir, mut database) = open_nullable("nulls");
    let null_rows = database
        .execute("SELECT * FROM t WHERE a IS NULL")
        .expect("is null");
    assert_eq!(null_rows.len(), 1);
    assert_eq!(null_rows[0].get("a"), Some(&ScalarValue::Null));

    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a IS NOT NULL")
            .expect("is not null")
            .len(),
        2
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a = NULL")
            .expect("eq null")
            .len(),
        0,
        "= NULL es UNKNOWN, no TRUE"
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE b IS NULL AND a = 1")
            .expect("and unknown")
            .len(),
        1
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE b = 2 OR a IS NULL")
            .expect("or null")
            .len(),
        2
    );
}

/// AC-0183-03: `SELECT DISTINCT` deduplica 800 filas sin O(n²).
#[test] // @spec AC-0183-03
fn test_ac_0183_03_distinct_linear() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("distinct.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "d",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create");
    for index in 0..800i64 {
        database
            .insert(
                "d",
                ScalarMap::from([(
                    "a".to_string(),
                    if index % 7 == 0 {
                        ScalarValue::Null
                    } else {
                        ScalarValue::Int(index % 10)
                    },
                )]),
            )
            .expect("insert");
    }
    let rows = database
        .execute("SELECT DISTINCT a FROM d")
        .expect("distinct");
    assert_eq!(rows.len(), 11, "0..=9 + NULL");
}

/// AC-0184-01: `prepare` + `execute_prepared` reutilizan el plan.
#[test] // @spec AC-0184-01
fn test_ac_0184_01_prepare_execute() {
    let (_dir, mut database) = open_nullable("prepared");
    let handle = database
        .prepare("SELECT * FROM t WHERE a = ?")
        .expect("prepare");
    assert_eq!(handle.param_count(), 1);
    let rows = database
        .execute_prepared(&handle, &[ParamValue::Int(1)])
        .expect("execute");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("a"), Some(&ScalarValue::Int(1)));

    let typed = database
        .prepare_typed("SELECT * FROM t WHERE a = $1", vec![ParamType::Int])
        .expect("typed");
    let typed_again = database
        .prepare_typed("SELECT * FROM t WHERE a = $1", vec![ParamType::Int])
        .expect("typed again");
    assert_eq!(typed.plan_cache_key(), typed_again.plan_cache_key());
    let rows = database
        .execute_prepared(&typed, &[ParamValue::Int(2)])
        .expect("execute typed");
    assert_eq!(rows.len(), 1);
}

/// AC-0184-02: los códigos de error son estables.
#[test] // @spec AC-0184-02
fn test_ac_0184_02_error_codes() {
    assert_eq!(
        RuscaError::ParseError {
            message: "x".to_string(),
            position: 0
        }
        .code(),
        "RQL_PARSE"
    );
    assert_eq!(
        RuscaError::ColumnNotFound {
            column: "x".to_string()
        }
        .code(),
        "COLUMN_NOT_FOUND"
    );
    assert_eq!(
        RuscaError::TypeMismatch {
            message: "x".to_string()
        }
        .code(),
        "TYPE_MISMATCH"
    );
    assert_eq!(
        RuscaError::ReadOnly {
            operation: "insert".to_string()
        }
        .code(),
        "READ_ONLY"
    );
    assert_eq!(
        RuscaError::WriteConflict {
            key: "k".to_string()
        }
        .code(),
        "WRITE_CONFLICT"
    );
}

/// AC-0184-03: aridad/tipo inválidos fallan antes de ejecutar SQL.
#[test] // @spec AC-0184-03
fn test_ac_0184_03_bind_errors() {
    let (_dir, mut database) = open_nullable("bind_errors");
    let handle = database.prepare("SELECT * FROM t WHERE a = ?").expect("p");
    let error = database.execute_prepared(&handle, &[]).expect_err("aridad");
    assert_eq!(error.code(), "INVALID_CONFIG");
    assert!(error.to_string().contains("parametros"), "{error}");

    let typed = database
        .prepare_typed("SELECT * FROM t WHERE a = $1", vec![ParamType::Int])
        .expect("typed");
    let error = database
        .execute_prepared(&typed, &[ParamValue::Text("no".to_string())])
        .expect_err("tipo");
    assert_eq!(error.code(), "INVALID_CONFIG");

    let error = database.prepare("").expect_err("vacia");
    assert_eq!(error.code(), "INVALID_CONFIG");
}
