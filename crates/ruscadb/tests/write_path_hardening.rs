//! Tests de aceptación del endurecimiento del camino de escritura (SPEC-0070).
//!
//! Cubre los criterios AC-0070-01/02/04: `insert_many` (camino real del
//! `INSERT` SQL) respeta RBAC y aplica los gates de ingesta, `Database::delete`
//! exige rol `Write`, y el audit usa un reloj monótono con nanos estrictamente
//! crecientes.
//!
//! Estos tests son **RED por aserción** (no por símbolos): la API pública que
//! usan ya existe en la fachada `ruscadb`, por lo que la suite compila, pero
//! hoy `insert_many` evade RBAC/gates, `delete` no comprueba el rol y el audit
//! puede repetir nanos. Cada aserción reproduce un hallazgo P1/P2 de la
//! retrospectiva 3.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, IngestionPolicy, Record, RecordId,
    RecordMeta, Role, RuscaError, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool de los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Nombre de la tabla usada en todos los escenarios.
const TABLE: &str = "items";

/// Tope nominal de bytes por registro usado para ejercer el gate de tamaño.
const MAX_RECORD_BYTES: usize = 1024;

/// Relleno del documento grande: supera con holgura el tope del gate (1024) y
/// cabe en una página del heap (PAGE_SIZE = 4096 sin overflow pages).
const LARGE_DOC_PADDING: usize = 2048;

/// Número de registros del lote sembrado en el escenario RBAC.
const BATCH_LEN: usize = 2;

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
    let database =
        Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura de la base");
    (dir, database)
}

/// Crea `items(id INT, name TEXT)`.
///
/// Args:
///     database: Base destino.
///     table: Nombre de la tabla a crear.
fn create_items(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
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

/// Construye escalares `(id, name)`.
///
/// Args:
///     id: Valor entero de la columna `id`.
///     name: Valor textual de la columna `name`.
///
/// Returns:
///     El mapa de escalares listo para un registro.
fn scalars(id: i64, name: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ])
}

/// Construye un documento JSON grande (supera con holgura `MAX_RECORD_BYTES`).
///
/// Returns:
///     El texto JSON `{"payload":"<LARGE_DOC_PADDING caracteres>"}`.
fn large_doc_json() -> String {
    format!(r#"{{"payload":"{}"}}"#, "A".repeat(LARGE_DOC_PADDING))
}

/// Construye un [`Record`] con escalares y documento opcional.
///
/// Args:
///     id: Identificador del registro.
///     scalars: Escalares del registro.
///     doc_json: Documento JSON como texto (`None` = sin documento); se parsea
///         al tipo `serde_json::Value` del campo `doc` sin nombrar el crate.
///
/// Returns:
///     El registro listo para `insert_many`.
fn record(id: RecordId, scalars: ScalarMap, doc_json: Option<&str>) -> Record {
    Record {
        id,
        scalars,
        doc: doc_json.map(|text| text.parse().expect("documento JSON válido")),
        edges: EdgeSet::default(),
        vector: None,
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Extrae el `name` de la fila cuyo `id` coincide.
///
/// Args:
///     rows: Filas devueltas por `execute`.
///     id: Identificador buscado.
///
/// Returns:
///     El nombre de la fila si existe y es `Text`.
fn name_of(rows: &[ruscadb::Row], id: i64) -> Option<String> {
    rows.iter()
        .find(|row| row.get("id") == Some(&ScalarValue::Int(id)))
        .and_then(|row| match row.get("name") {
            Some(ScalarValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
}

/// Afirma que el resultado es `PermissionDenied` con acción accionable.
///
/// Comprueba la variante tipada, que la acción no esté vacía y que el mensaje
/// humano contenga dicha acción (WHAT + WHY + WHERE).
///
/// Args:
///     result: Resultado de la operación potencialmente denegada.
///     context: Etiqueta del escenario para el reporte de fallo.
///
/// Returns:
///     La acción denegada extraída del error.
///
/// Raises:
///     Panic si el resultado no es `PermissionDenied`.
fn assert_permission_denied<T: std::fmt::Debug>(
    result: Result<T, RuscaError>,
    context: &str,
) -> String {
    let err = match result {
        Err(err) => err,
        Ok(value) => panic!("{context}: se esperaba PermissionDenied, se obtuvo Ok({value:?})"),
    };
    assert!(
        matches!(err, RuscaError::PermissionDenied { .. }),
        "{context}: se esperaba RuscaError::PermissionDenied, se obtuvo {err:?}"
    );
    let action = match &err {
        RuscaError::PermissionDenied { action } => action.clone(),
        other => panic!("{context}: variante inesperada {other:?}"),
    };
    assert!(
        !action.is_empty(),
        "{context}: la acción denegada no debe ser vacía"
    );
    assert!(
        err.to_string().contains(&action),
        "{context}: el mensaje debe contener la acción '{action}': {err}"
    );
    action
}

/// AC-0070-01 — `insert_many` respeta RBAC y aplica el gate de tamaño.
///
/// (a) Con rol `Read` el lote se deniega con `PermissionDenied` y no deja
/// filas. (b) Con `Admin` y `max_record_bytes = 1024` un registro cuyo payload
/// supera el tope se rechaza con `ResourceLimit` en el camino de `insert_many`.
#[test]
// @spec AC-0070-01
fn test_ac_0070_01_insert_many_respects_rbac_and_gates() {
    // (a) RBAC: el lote real del SQL INSERT no debe evadir el rol.
    let (_dir_rbac, mut database) = open_test_db("ac0070_01_rbac");
    create_items(&mut database, TABLE);
    database.set_role(Role::Read).expect("set_role(Read)");
    assert_eq!(database.current_role(), Role::Read, "el rol debe ser Read");

    let batch = vec![
        record(RecordId::new(), scalars(1, "uno"), None),
        record(RecordId::new(), scalars(2, "dos"), None),
    ];
    assert_eq!(
        batch.len(),
        BATCH_LEN,
        "el lote de prueba tiene 2 registros"
    );
    assert_permission_denied(
        database.insert_many(TABLE, batch),
        "Read insert_many (camino del INSERT SQL)",
    );

    let rows = database
        .execute("SELECT * FROM items")
        .expect("SELECT con rol Read");
    assert!(
        rows.is_empty(),
        "el lote denegado no debe publicar filas: {} filas",
        rows.len()
    );

    // (b) Gate de tamaño: `insert_many` debe aplicar los gates por registro.
    let (_dir_gate, mut gated) = open_test_db("ac0070_01_gate");
    gated.set_ingestion_policy(IngestionPolicy {
        max_record_bytes: MAX_RECORD_BYTES,
        require_registry: false,
    });
    assert_eq!(
        gated.ingestion_policy().max_record_bytes,
        MAX_RECORD_BYTES,
        "la política fijada debe leerse de vuelta"
    );
    create_items(&mut gated, TABLE);

    let doc = large_doc_json();
    let oversized = record(RecordId::new(), scalars(1, "grande"), Some(&doc));
    let error = gated
        .insert_many(TABLE, vec![oversized])
        .expect_err("un registro sobredimensionado en insert_many debe rechazarse");
    assert!(
        matches!(error, RuscaError::ResourceLimit { .. }),
        "se esperaba ResourceLimit en el camino de insert_many, se obtuvo {error:?}"
    );
    assert!(
        error.to_string().contains(&MAX_RECORD_BYTES.to_string()),
        "el error debe citar el tope configurado ({MAX_RECORD_BYTES}): {error}"
    );

    let rows = gated
        .execute("SELECT * FROM items")
        .expect("SELECT tras el rechazo");
    assert!(
        rows.is_empty(),
        "el rechazo por tamaño no debe dejar la fila: {} filas",
        rows.len()
    );
}

/// AC-0070-02 — `Database::delete` exige rol `Write` y no borra con `Read`.
///
/// Con una fila existente y rol `Read`, el borrado devuelve `PermissionDenied`
/// y la fila sigue viva para el `SELECT` posterior.
#[test]
// @spec AC-0070-02
fn test_ac_0070_02_delete_requires_write_role() {
    let (_dir, mut database) = open_test_db("ac0070_02");
    create_items(&mut database, TABLE);
    let id = database
        .insert(TABLE, scalars(1, "vivo"))
        .expect("insert de la fila sembrada");

    database.set_role(Role::Read).expect("set_role(Read)");
    assert_permission_denied(database.delete(TABLE, &id), "Read delete");

    let rows = database
        .execute("SELECT * FROM items")
        .expect("SELECT con rol Read");
    assert_eq!(
        rows.len(),
        1,
        "el borrado denegado no debe eliminar la fila"
    );
    assert_eq!(
        name_of(&rows, 1),
        Some("vivo".to_string()),
        "la fila debe conservar su contenido original"
    );
}

/// AC-0070-04 — el audit usa un reloj monótono con nanos estrictamente
/// crecientes entre operaciones consecutivas.
#[test]
// @spec AC-0070-04
fn test_ac_0070_04_audit_nanos_strictly_increase() {
    let (_dir, mut database) = open_test_db("ac0070_04");
    create_items(&mut database, TABLE);
    database.set_audit(true).expect("set_audit(true)");

    database
        .insert(TABLE, scalars(1, "uno"))
        .expect("insert auditado 1");
    database
        .insert(TABLE, scalars(2, "dos"))
        .expect("insert auditado 2");
    database
        .insert(TABLE, scalars(3, "tres"))
        .expect("insert auditado 3");

    let entries = database.audit_trail();
    assert!(
        entries.len() >= 3,
        "la batería debe registrar al menos 3 entradas: {}",
        entries.len()
    );
    for pair in entries.windows(2) {
        assert!(
            pair[0].nanos < pair[1].nanos,
            "los nanos del audit deben ser estrictamente crecientes: {} < {}",
            pair[0].nanos,
            pair[1].nanos
        );
    }
}
