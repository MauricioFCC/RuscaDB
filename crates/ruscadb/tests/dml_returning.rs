//! Tests de aceptación de `RETURNING` en DML (SPEC-0102).
//!
//! Cubre AC-0102-01 (`INSERT ... RETURNING` devuelve las filas insertadas) y
//! AC-0102-02 (`UPDATE`/`DELETE ... RETURNING` devuelven las filas afectadas).
//! El parseo/canonización (AC-0102-03) vive en las pruebas unitarias de
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

/// Crea la tabla `t(id INT, name TEXT)`.
fn create_t(database: &mut Database) {
    database
        .create_table(
            "t",
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Extrae `(id, name)` de una fila.
fn pair(row: &Row) -> (i64, String) {
    let id = match row.get("id") {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en 'id', se obtuvo {other:?}"),
    };
    let name = match row.get("name") {
        Some(ScalarValue::Text(value)) => value.clone(),
        other => panic!("se esperaba Text en 'name', se obtuvo {other:?}"),
    };
    (id, name)
}

/// AC-0102-01 — `INSERT ... RETURNING` devuelve las filas insertadas.
#[test] // @spec AC-0102-01
fn test_ac_0102_01_insert_returning_rows() {
    let (_dir, mut database) = open_test_db("ac0102_01");
    create_t(&mut database);

    let rows = database
        .execute("INSERT INTO t (id, name) VALUES (1, 'uno'), (2, 'dos') RETURNING id, name")
        .expect("insert returning");
    assert_eq!(rows.len(), 2, "RETURNING devuelve una fila por insertado");
    assert_eq!(pair(&rows[0]), (1, "uno".to_string()));
    assert_eq!(pair(&rows[1]), (2, "dos".to_string()));
    assert!(
        !rows[0].contains_key("affected"),
        "con RETURNING no se devuelve el contador affected"
    );

    // `RETURNING *` incluye todas las columnas del esquema.
    let all = database
        .execute("INSERT INTO t (id, name) VALUES (3, 'tres') RETURNING *")
        .expect("insert returning *");
    assert_eq!(all.len(), 1);
    assert_eq!(pair(&all[0]), (3, "tres".to_string()));

    // Sin RETURNING se conserva `{"affected": N}` (cero regresión).
    let affected = database
        .execute("INSERT INTO t (id, name) VALUES (4, 'cuatro')")
        .expect("insert sin returning");
    assert_eq!(
        affected[0].get("affected"),
        Some(&ScalarValue::Int(1)),
        "sin RETURNING debe devolverse affected"
    );
}

/// AC-0102-02 — `UPDATE`/`DELETE ... RETURNING` devuelven las filas afectadas.
#[test] // @spec AC-0102-02
fn test_ac_0102_02_update_delete_returning_rows() {
    let (_dir, mut database) = open_test_db("ac0102_02");
    create_t(&mut database);
    database
        .execute("INSERT INTO t (id, name) VALUES (1, 'uno'), (2, 'dos')")
        .expect("seed");

    let updated = database
        .execute("UPDATE t SET name = 'DOS' WHERE id = 2 RETURNING id, name")
        .expect("update returning");
    assert_eq!(updated.len(), 1, "una fila actualizada");
    assert_eq!(
        pair(&updated[0]),
        (2, "DOS".to_string()),
        "RETURNING refleja el valor nuevo"
    );

    let deleted = database
        .execute("DELETE FROM t WHERE id = 1 RETURNING id, name")
        .expect("delete returning");
    assert_eq!(deleted.len(), 1, "una fila borrada");
    assert_eq!(
        pair(&deleted[0]),
        (1, "uno".to_string()),
        "RETURNING devuelve los valores de la fila borrada"
    );

    // Tras el borrado, solo queda la fila actualizada.
    let remaining = database.execute("SELECT * FROM t").expect("select final");
    assert_eq!(remaining.len(), 1);
    assert_eq!(pair(&remaining[0]), (2, "DOS".to_string()));

    // `DELETE ... RETURNING` sin coincidencias devuelve 0 filas (no error).
    let none = database
        .execute("DELETE FROM t WHERE id = 99 RETURNING id")
        .expect("delete returning vacío");
    assert!(none.is_empty(), "sin coincidencias RETURNING queda vacío");
}
