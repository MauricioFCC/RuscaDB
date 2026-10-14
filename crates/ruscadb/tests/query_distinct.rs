//! Tests de aceptación de `SELECT DISTINCT` (SPEC-0097).
//!
//! Cubre AC-0097-01 (dedupe de la proyección preservando el orden de primera
//! aparición, con y sin `ORDER BY`) y AC-0097-02 (el `DISTINCT` se aplica antes
//! de `LIMIT`/`OFFSET`). El parseo/canonización (AC-0097-03) vive en las
//! pruebas unitarias del crate `ruscadb-query`.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, ScalarValue};

/// Abre una base temporal de pruebas con pool amplio.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 64)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `t(a INT, b TEXT)`.
fn create_t(database: &mut Database) {
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
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Siembra `t` con filas que repiten valores en `a` y en `b`.
fn seed(database: &mut Database) {
    database
        .execute("INSERT INTO t (a, b) VALUES (1, 'x'), (1, 'x'), (2, 'y'), (1, 'y'), (2, 'y')")
        .expect("insert");
}

/// Extrae la columna `a` como `Vec<i64>`.
fn column_a(rows: &[Row]) -> Vec<i64> {
    rows.iter()
        .map(|row| match row.get("a") {
            Some(ScalarValue::Int(value)) => *value,
            other => panic!("se esperaba Int en 'a', se obtuvo {other:?}"),
        })
        .collect()
}

/// Extrae la columna `b` como `Vec<String>`.
fn column_b(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|row| match row.get("b") {
            Some(ScalarValue::Text(value)) => value.clone(),
            other => panic!("se esperaba Text en 'b', se obtuvo {other:?}"),
        })
        .collect()
}

/// AC-0097-01 — `DISTINCT` deduplica la proyección preservando la 1.ª aparición.
#[test] // @spec AC-0097-01
fn test_ac_0097_01_distinct_dedups_projection() {
    let (_dir, mut database) = open_test_db("ac0097_01");
    create_t(&mut database);
    seed(&mut database);

    // Sin ORDER BY: orden de primera aparición.
    let a = database
        .execute("SELECT DISTINCT a FROM t")
        .expect("distinct a");
    assert_eq!(
        column_a(&a),
        [1, 2],
        "DISTINCT a deduplica en 1.ª aparición"
    );

    let b = database
        .execute("SELECT DISTINCT b FROM t")
        .expect("distinct b");
    assert_eq!(
        column_b(&b),
        ["x", "y"],
        "DISTINCT b deduplica en 1.ª aparición"
    );

    // `DISTINCT *` sobre pares: 3 combinaciones únicas (1,x), (2,y), (1,y),
    // en orden de primera aparición.
    let all = database
        .execute("SELECT DISTINCT * FROM t")
        .expect("distinct *");
    assert_eq!(all.len(), 3, "DISTINCT * deja 3 filas únicas");
    assert_eq!(
        column_a(&all),
        [1, 2, 1],
        "el orden de 1.ª aparición se conserva"
    );
    assert_eq!(
        column_b(&all),
        ["x", "y", "y"],
        "la proyección completa también se conserva"
    );

    // Con ORDER BY, el orden manda (DESC) y el dedupe sigue aplicando.
    let desc = database
        .execute("SELECT DISTINCT a FROM t ORDER BY a DESC")
        .expect("distinct desc");
    assert_eq!(column_a(&desc), [2, 1], "DISTINCT + ORDER BY DESC");
}

/// AC-0097-02 — `DISTINCT` se aplica antes de `LIMIT`/`OFFSET`.
#[test] // @spec AC-0097-02
fn test_ac_0097_02_distinct_before_pagination() {
    let (_dir, mut database) = open_test_db("ac0097_02");
    create_t(&mut database);
    seed(&mut database);

    // Sin DISTINCT, la ventana [1,1) sobre (1,1,1,2,2) daría [1].
    let windowed = database
        .execute("SELECT DISTINCT a FROM t ORDER BY a LIMIT 1 OFFSET 1")
        .expect("distinct paginado");
    assert_eq!(
        column_a(&windowed),
        [2],
        "sobre el resultado único [1, 2], OFFSET 1 LIMIT 1 deja [2]"
    );

    // Sin DISTINCT el mismo plan de ventana devuelve el duplicado.
    let plain = database
        .execute("SELECT a FROM t ORDER BY a LIMIT 1 OFFSET 1")
        .expect("paginado sin distinct");
    assert_eq!(
        column_a(&plain),
        [1],
        "sin DISTINCT la ventana cae sobre el duplicado"
    );
}
