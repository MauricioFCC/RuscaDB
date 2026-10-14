//! Tests de aceptación de la paginación `LIMIT ... OFFSET` (SPEC-0096).
//!
//! Cubre AC-0096-01 (ventana correcta sobre `ORDER BY`, con bordes: offset 0,
//! offset == filas, offset > filas, `LIMIT 0`) y AC-0096-02 (el `OFFSET`
//! también aplica a la ruta de agregación `GROUP BY`). El parseo/canonización y
//! los errores ruidosos (AC-0096-03) viven en las pruebas unitarias del crate
//! `ruscadb-query`.

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

/// Siembra `t` con las filas `(1..=n, "fila{i}")`.
fn seed(database: &mut Database, n: i64) {
    for value in 1..=n {
        database
            .execute(&format!(
                "INSERT INTO t (a, b) VALUES ({value}, 'fila{value}')"
            ))
            .expect("insert");
    }
}

/// Extrae la columna `a` de cada fila como `Vec<i64>`.
fn column_a(rows: &[Row]) -> Vec<i64> {
    rows.iter()
        .map(|row| match row.get("a") {
            Some(ScalarValue::Int(value)) => *value,
            other => panic!("se esperaba Int en 'a', se obtuvo {other:?}"),
        })
        .collect()
}

/// AC-0096-01 — `LIMIT`/`OFFSET` devuelven la ventana correcta sobre `ORDER BY`.
#[test] // @spec AC-0096-01
fn test_ac_0096_01_limit_offset_window() {
    let (_dir, mut database) = open_test_db("ac0096_01");
    create_t(&mut database);
    seed(&mut database, 5);

    // Primera página (offset 0) ≡ sin OFFSET.
    let first = database
        .execute("SELECT * FROM t ORDER BY a LIMIT 2 OFFSET 0")
        .expect("página 1");
    assert_eq!(column_a(&first), [1, 2], "offset 0 devuelve el inicio");

    // Segunda página.
    let second = database
        .execute("SELECT * FROM t ORDER BY a LIMIT 2 OFFSET 2")
        .expect("página 2");
    assert_eq!(
        column_a(&second),
        [3, 4],
        "offset 2 salta la primera página"
    );

    // OFFSET sin LIMIT: descarta el prefijo (semántica Postgres).
    let tail = database
        .execute("SELECT * FROM t ORDER BY a OFFSET 4")
        .expect("offset sin limit");
    assert_eq!(column_a(&tail), [5], "offset 4 deja la última fila");

    // Bordes: offset == filas y offset > filas ⇒ vacío; LIMIT 0 ⇒ vacío.
    for query in [
        "SELECT * FROM t ORDER BY a OFFSET 5",
        "SELECT * FROM t ORDER BY a LIMIT 2 OFFSET 10",
        "SELECT * FROM t ORDER BY a LIMIT 0 OFFSET 0",
    ] {
        let rows = database.execute(query).expect("consulta de borde");
        assert!(
            rows.is_empty(),
            "'{query}' debe devolver 0 filas, devolvió {}",
            rows.len()
        );
    }
}

/// AC-0096-02 — `OFFSET` también se aplica a la ruta de agregación `GROUP BY`.
#[test] // @spec AC-0096-02
fn test_ac_0096_02_offset_applies_to_group_by() {
    let (_dir, mut database) = open_test_db("ac0096_02");
    create_t(&mut database);
    // Dos grupos (`a` ∈ {1, 2}); el orden canónico es ascendente.
    database
        .execute("INSERT INTO t (a, b) VALUES (1, 'x'), (1, 'x'), (2, 'y')")
        .expect("insert");

    let grouped = database
        .execute("SELECT a, COUNT(*) FROM t GROUP BY a ORDER BY a LIMIT 1 OFFSET 1")
        .expect("group by con offset");
    assert_eq!(grouped.len(), 1, "se pagina un único grupo");
    assert_eq!(
        grouped[0].get("a"),
        Some(&ScalarValue::Int(2)),
        "offset 1 salta el grupo a=1"
    );
    assert_eq!(
        grouped[0].get("count"),
        Some(&ScalarValue::Int(1)),
        "el grupo a=2 tiene una fila"
    );
}
