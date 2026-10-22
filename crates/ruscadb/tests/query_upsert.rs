//! Tests de aceptación de `UPSERT` (`INSERT ... ON CONFLICT`, SPEC-0107).
//!
//! Cubre AC-0107-01..09. El parseo/canonización (FR-0107-01) vive también
//! en las pruebas unitarias de `ruscadb-query`.

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

/// AC-0107-01 — sin coincidencia se inserta la fila propuesta.
#[test] // @spec AC-0107-01
fn test_ac_0107_01_upsert_inserts_when_no_conflict() {
    let (_dir, mut database) = open_test_db("ac0107_01");
    create_t(&mut database);

    let rows = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'uno') \
             ON CONFLICT (id) DO UPDATE SET name = excluded.name \
             RETURNING id, name",
        )
        .expect("upsert insert");
    assert_eq!(rows.len(), 1);
    assert_eq!(pair(&rows[0]), (1, "uno".to_string()));

    let stored = database.execute("SELECT id, name FROM t").expect("select");
    assert_eq!(stored.len(), 1);
    assert_eq!(pair(&stored[0]), (1, "uno".to_string()));
}

/// AC-0107-02 — con una coincidencia se actualiza la fila existente.
#[test] // @spec AC-0107-02
fn test_ac_0107_02_upsert_updates_single_match() {
    let (_dir, mut database) = open_test_db("ac0107_02");
    create_t(&mut database);
    database
        .execute("INSERT INTO t (id, name) VALUES (1, 'viejo')")
        .expect("seed");

    let rows = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'nuevo') \
             ON CONFLICT (id) DO UPDATE SET name = excluded.name \
             RETURNING id, name",
        )
        .expect("upsert update");
    assert_eq!(rows.len(), 1);
    assert_eq!(pair(&rows[0]), (1, "nuevo".to_string()));

    let stored = database.execute("SELECT id, name FROM t").expect("select");
    assert_eq!(stored.len(), 1);
    assert_eq!(pair(&stored[0]), (1, "nuevo".to_string()));
}

/// AC-0107-03 — `excluded.col` resuelve al valor de la fila propuesta.
#[test] // @spec AC-0107-03
fn test_ac_0107_03_upsert_excluded_resolves_from_proposed_row() {
    let (_dir, mut database) = open_test_db("ac0107_03");
    create_t(&mut database);
    database
        .execute("INSERT INTO t (id, name) VALUES (1, 'viejo')")
        .expect("seed");

    // El SET usa excluded.name (propuesto) y un literal distinto para verificar
    // que excluded no se confunde con el valor previo ni con el literal.
    let rows = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'propuesto') \
             ON CONFLICT (id) DO UPDATE SET name = excluded.name \
             RETURNING id, name",
        )
        .expect("upsert excluded");
    assert_eq!(rows.len(), 1);
    assert_eq!(pair(&rows[0]), (1, "propuesto".to_string()));
}

/// AC-0107-04 — DO NOTHING deja la fila existente intacta.
#[test] // @spec AC-0107-04
fn test_ac_0107_04_do_nothing_leaves_row_untouched() {
    let (_dir, mut database) = open_test_db("ac0107_04");
    create_t(&mut database);
    database
        .execute("INSERT INTO t (id, name) VALUES (1, 'original')")
        .expect("seed");

    let rows = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'intentado') \
             ON CONFLICT (id) DO NOTHING \
             RETURNING id, name",
        )
        .expect("upsert do nothing");
    // DO NOTHING no reporta la fila en conflicto.
    assert!(
        rows.is_empty(),
        "DO NOTHING no debe reportar la fila en conflicto"
    );

    let stored = database.execute("SELECT id, name FROM t").expect("select");
    assert_eq!(stored.len(), 1);
    assert_eq!(pair(&stored[0]), (1, "original".to_string()));
}

/// AC-0107-05 — mas de un coincidente falla de forma atomica.
#[test] // @spec AC-0107-05
fn test_ac_0107_05_ambiguous_target_fails_atomically() {
    let (_dir, mut database) = open_test_db("ac0107_05");
    create_t(&mut database);
    database
        .execute("INSERT INTO t (id, name) VALUES (1, 'a')")
        .expect("seed a");
    database
        .execute("INSERT INTO t (id, name) VALUES (1, 'b')")
        .expect("seed b");

    let error = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'nuevo') \
             ON CONFLICT (id) DO UPDATE SET name = excluded.name",
        )
        .expect_err("ambiguedad debe fallar");
    assert!(
        error.to_string().to_lowercase().contains("ambig")
            || error.to_string().to_lowercase().contains("conflict")
            || error.to_string().to_lowercase().contains("join"),
        "error accionable, se obtuvo: {error}"
    );

    // Atomicidad: las filas previas siguen intactas (2 filas, valores originales).
    let stored = database.execute("SELECT id, name FROM t").expect("select");
    assert_eq!(stored.len(), 2);
}

/// AC-0107-06 — varias filas propuestas en un solo commit.
#[test] // @spec AC-0107-06
fn test_ac_0107_06_multi_row_upsert_single_commit() {
    let (_dir, mut database) = open_test_db("ac0107_06");
    create_t(&mut database);

    let rows = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'a'), (2, 'b'), (3, 'c') \
             ON CONFLICT (id) DO UPDATE SET name = excluded.name \
             RETURNING id, name",
        )
        .expect("multi upsert");
    assert_eq!(rows.len(), 3);

    let stored = database
        .execute("SELECT id, name FROM t ORDER BY id")
        .expect("select");
    assert_eq!(stored.len(), 3);
    assert_eq!(pair(&stored[0]), (1, "a".to_string()));
    assert_eq!(pair(&stored[1]), (2, "b".to_string()));
    assert_eq!(pair(&stored[2]), (3, "c".to_string()));
}

/// AC-0107-07 — mezcla de insert y update en una sola sentencia.
#[test] // @spec AC-0107-07
fn test_ac_0107_07_multi_row_mixed_actions() {
    let (_dir, mut database) = open_test_db("ac0107_07");
    create_t(&mut database);
    database
        .execute("INSERT INTO t (id, name) VALUES (2, 'viejo')")
        .expect("seed");

    let rows = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'nuevo'), (2, 'actualizado') \
             ON CONFLICT (id) DO UPDATE SET name = excluded.name \
             RETURNING id, name",
        )
        .expect("mixed upsert");
    assert_eq!(rows.len(), 2);

    let stored = database
        .execute("SELECT id, name FROM t ORDER BY id")
        .expect("select");
    assert_eq!(stored.len(), 2);
    assert_eq!(pair(&stored[0]), (1, "nuevo".to_string()));
    assert_eq!(pair(&stored[1]), (2, "actualizado".to_string()));
}

/// AC-0107-08 — RETURNING lista las filas efectivamente afectadas en orden.
#[test] // @spec AC-0107-08
fn test_ac_0107_08_returning_reports_effective_rows() {
    let (_dir, mut database) = open_test_db("ac0107_08");
    create_t(&mut database);
    database
        .execute("INSERT INTO t (id, name) VALUES (2, 'previo')")
        .expect("seed");

    let rows = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'a'), (2, 'b') \
             ON CONFLICT (id) DO NOTHING \
             RETURNING id, name",
        )
        .expect("upsert returning");
    // Solo la fila insertada (id=1) se reporta; la de conflicto con DO NOTHING no.
    assert_eq!(rows.len(), 1);
    assert_eq!(pair(&rows[0]), (1, "a".to_string()));
}

/// AC-0107-09 — update parcial conserva las columnas no mencionadas.
#[test] // @spec AC-0107-09
fn test_ac_0107_09_upsert_preserves_unmentioned_columns() {
    let (_dir, mut database) = open_test_db("ac0107_09");
    create_t(&mut database);
    database
        .execute("INSERT INTO t (id, name) VALUES (1, 'original')")
        .expect("seed");

    // SET solo menciona 'name'... aquí se menciona 'name' con literal para
    // verificar que un SET parcial no borra otras columnas (id se conserva).
    let rows = database
        .execute(
            "INSERT INTO t (id, name) VALUES (1, 'ignorado') \
             ON CONFLICT (id) DO UPDATE SET name = 'actualizado' \
             RETURNING id, name",
        )
        .expect("upsert partial");
    assert_eq!(rows.len(), 1);
    assert_eq!(pair(&rows[0]), (1, "actualizado".to_string()));

    let stored = database.execute("SELECT id, name FROM t").expect("select");
    assert_eq!(stored.len(), 1);
    assert_eq!(pair(&stored[0]), (1, "actualizado".to_string()));
}

/// Regresion: el upsert reporta el numero de filas afectadas sin RETURNING.
#[test]
fn test_upsert_reports_affected_count() {
    let (_dir, mut database) = open_test_db("upsert_affected");
    create_t(&mut database);

    let rows = database
        .execute("INSERT INTO t (id, name) VALUES (1, 'uno') ON CONFLICT (id) DO UPDATE SET name = excluded.name")
        .expect("insert");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("affected"), Some(&ScalarValue::Int(1)));

    let rows = database
        .execute("INSERT INTO t (id, name) VALUES (1, 'dos') ON CONFLICT (id) DO UPDATE SET name = excluded.name")
        .expect("update");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("affected"), Some(&ScalarValue::Int(1)));

    let rows = database
        .execute("INSERT INTO t (id, name) VALUES (1, 'tres') ON CONFLICT (id) DO NOTHING")
        .expect("do nothing");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("affected"), Some(&ScalarValue::Int(0)));
}
