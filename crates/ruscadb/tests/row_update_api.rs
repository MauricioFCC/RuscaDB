//! Test de aceptación de la API estructurada de actualización (SPEC-0091).
//!
//! Cubre AC-0091-01: `Database::update(table, id, scalars)` fusiona columnas
//! parciales sobre una fila viva, conserva las no indicadas y persiste el
//! cambio de forma visible para un `SELECT` posterior.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 64)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `items(id INT, name TEXT)`.
///
/// Args:
///     database: Base destino.
fn create_items(database: &mut Database) {
    database
        .create_table(
            "items",
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

/// Construye los escalares `(id, name)` de una fila.
///
/// Args:
///     id: Valor entero de `id`.
///     name: Valor textual de `name`.
///
/// Returns:
///     El mapa de escalares listo para `insert`.
fn row_scalars(id: i64, name: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ])
}

/// AC-0091-01 — `update` fusiona columnas parciales y persiste el cambio.
#[test] // @spec AC-0091-01
fn test_ac_0091_01_update_facade_persists_partial_change() {
    let (_dir, mut database) = open_test_db("ac0091_01");
    create_items(&mut database);
    let id = database
        .insert("items", row_scalars(1, "uno"))
        .expect("insert de la fila sembrada");

    let changed = database
        .update(
            "items",
            &id,
            ScalarMap::from([("name".to_string(), ScalarValue::Text("dos".to_string()))]),
        )
        .expect("update de la columna name");
    assert!(changed, "la fila viva debe quedar actualizada");

    // El cambio es visible para el mismo motor de consulta de la fachada.
    let rows = database.execute("SELECT * FROM items").expect("select");
    assert_eq!(rows.len(), 1, "la actualización no debe duplicar filas");
    assert_eq!(
        rows[0].get("name"),
        Some(&ScalarValue::Text("dos".to_string())),
        "el SELECT debe ver el valor nuevo"
    );
    assert_eq!(
        rows[0].get("id"),
        Some(&ScalarValue::Int(1)),
        "la columna no indicada debe conservar su valor"
    );

    // Idempotencia: un `RecordId` inexistente devuelve false sin error.
    let ghost = ruscadb::RecordId::new();
    let missing = database
        .update(
            "items",
            &ghost,
            ScalarMap::from([("name".to_string(), ScalarValue::Text("x".to_string()))]),
        )
        .expect("un id inexistente no debe fallar");
    assert!(!missing, "un id inexistente no actualiza ninguna fila");
    assert_eq!(
        database
            .execute("SELECT * FROM items")
            .expect("select final")
            .len(),
        1,
        "el intento sobre un id inexistente no debe crear ni borrar filas"
    );
}
