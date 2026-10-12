//! Tests de aceptación de audit + RBAC por rol de instancia (SPEC-0066).
//!
//! Cubre los criterios AC-0066-01..05: permisos por rol (`Read` no escribe,
//! `Write` no administra, `Admin` todo) con error tipado `PermissionDenied`, y
//! el audit en memoria (captura en orden, habilitación explícita y reseteo que
//! no deshabilita).
//!
//! Estos tests son **RED por diseño**: la API de SPEC-0066 aún no existe en la
//! fachada `ruscadb`, por lo que la compilación falla al resolver los símbolos
//! `Role`, `AuditEntry`, `Database::{set_role, current_role, set_audit,
//! audit_trail, clear_audit}` y la variante `RuscaError::PermissionDenied`.
//!
//! Nota de contrato: `set_role`, `set_audit` y `clear_audit` se invocan como
//! `Result<(), RuscaError>` porque AC-0066-02 exige que sus denegaciones lleguen
//! al host como `PermissionDenied` (no como una omisión silenciosa).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    AuditEntry, ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta,
    Metric, Record, RecordId, RecordMeta, Role, Row, RuscaError, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool de los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Nombre de la tabla usada en todos los escenarios.
const TABLE: &str = "items";

/// Dimensión de los vectores de prueba (coincide con la query KNN).
const VECTOR_DIM: usize = 3;

/// Valor de `k` de la consulta KNN de prueba.
const KNN_K: usize = 2;

/// Vector de consulta de las pruebas KNN.
const QUERY_VECTOR: [f64; VECTOR_DIM] = [0.1, 0.2, 0.3];

/// Número de filas vectoriales sembradas por [`seed_vector_rows`].
const SEEDED_VECTOR_ROWS: usize = 4;

/// Número de filas escalares sembradas por [`seed_scalar_rows`].
const SEEDED_SCALAR_ROWS: usize = 2;

/// Mínimo de entradas de audit exigidas a la batería de AC-0066-04.
const AUDIT_MIN_ENTRIES: usize = 3;

/// Payload usado en las operaciones `put_blob` de los tests.
const BLOB_PAYLOAD: &[u8] = b"contenido de auditoria rbac 0066";

/// Abre una base temporal de pruebas con pool amplio y blob store configurado.
///
/// El blob store se habilita porque varios escenarios ejercen `put_blob`/`gc_blobs`
/// y deben fallar (o triunfar) por permisos, nunca por configuración ausente.
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let blob_path = dir.path().join(format!("{tag}-blobs"));
    let mut config = DbConfig::new(&path, POOL_CAPACITY);
    config.blob_path = Some(blob_path);
    let database = Database::open(config).expect("apertura de la base");
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

/// Crea un embedding L2 de la dimensión del vector.
///
/// Args:
///     values: Componentes del vector en `f64`.
///
/// Returns:
///     El embedding L2 construido.
fn embedding(values: &[f64]) -> Embedding {
    let floats: Vec<f32> = values.iter().map(|value| *value as f32).collect();
    Embedding::new(
        floats,
        EmbeddingMeta {
            model_id: "test".to_string(),
            dim: values.len(),
            metric: Metric::L2,
        },
    )
    .expect("embedding válido")
}

/// Construye un `Record` con escalares y vector (sin aristas).
///
/// Args:
///     id: Identificador del registro.
///     scalars: Escalares del registro.
///     vector: Componentes del embedding.
///
/// Returns:
///     El registro listo para `insert_record`.
fn vector_record(id: RecordId, scalars: ScalarMap, vector: &[f64]) -> Record {
    Record {
        id,
        scalars,
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(embedding(vector)),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Construye la consulta KNN de prueba sobre `items`.
///
/// Returns:
///     El texto RQL `SELECT * FROM items KNN embedding <|k|> [...]`.
fn knn_query() -> String {
    format!(
        "SELECT * FROM items KNN embedding <|{KNN_K}|> [{}, {}, {}]",
        QUERY_VECTOR[0], QUERY_VECTOR[1], QUERY_VECTOR[2]
    )
}

/// Siembra `items` con filas escalares `(1, "origen")` y `(2, "cerca")`.
///
/// Args:
///     database: Base destino (debe tener la tabla `items`).
fn seed_scalar_rows(database: &mut Database) {
    database
        .insert(TABLE, scalars(1, "origen"))
        .expect("insert fila 1");
    database
        .insert(TABLE, scalars(2, "cerca"))
        .expect("insert fila 2");
}

/// Siembra `items` con filas vectoriales de ejemplo (para el KNN).
///
/// Args:
///     database: Base destino (debe tener la tabla `items`).
fn seed_vector_rows(database: &mut Database) {
    let data = [
        (1_i64, "origen", [0.0, 0.0, 0.0]),
        (2, "cerca", [0.1, 0.1, 0.1]),
        (3, "lejos", [9.0, 9.0, 9.0]),
        (4, "medio", [3.0, 3.0, 3.0]),
    ];
    for (id, name, vector) in data {
        let record = vector_record(RecordId::new(), scalars(id, name), &vector);
        database
            .insert_record(TABLE, record)
            .expect("insert_record vectorial");
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
fn name_of(rows: &[Row], id: i64) -> Option<String> {
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
/// humano contenga dicha acción (WHAT + WHY + WHERE, NF-0066-02).
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

/// Afirma que `prefix` es el prefijo exacto de `full` (append-only, sin reordenar).
///
/// Args:
///     prefix: Entradas capturadas antes.
///     full: Entradas capturadas después.
///     context: Etiqueta del escenario para el reporte de fallo.
fn assert_trail_prefix(prefix: &[AuditEntry], full: &[AuditEntry], context: &str) {
    assert!(
        full.len() >= prefix.len(),
        "{context}: el trail de audit no debe encogerse"
    );
    for (index, expected) in prefix.iter().enumerate() {
        let actual = &full[index];
        assert_eq!(
            actual.action, expected.action,
            "{context}: acción inesperada en posición {index}"
        );
        assert_eq!(
            actual.detail, expected.detail,
            "{context}: detalle inesperado en posición {index}"
        );
        assert_eq!(
            actual.nanos, expected.nanos,
            "{context}: nanos inesperados en posición {index}"
        );
    }
}

/// Afirma que las entradas de audit están bien formadas y ordenadas por tiempo.
///
/// Args:
///     entries: Trail de audit a validar.
fn assert_entries_well_formed(entries: &[AuditEntry]) {
    for (index, entry) in entries.iter().enumerate() {
        assert!(
            !entry.action.is_empty(),
            "entrada {index}: la acción no debe ser vacía"
        );
        assert!(
            entry.nanos > 0,
            "entrada {index}: el timestamp en nanos debe ser > 0"
        );
    }
    for window in entries.windows(2) {
        assert!(
            window[0].nanos <= window[1].nanos,
            "los timestamps del audit deben ser no decrecientes"
        );
    }
}

/// AC-0066-01 — con rol `Read` las escrituras se deniegan y no hay efectos.
#[test]
// @spec AC-0066-01
fn test_ac_0066_01_read_role_cannot_write() {
    let (_dir, mut database) = open_test_db("ac0066_01");
    create_items(&mut database, TABLE);
    seed_scalar_rows(&mut database);

    database.set_role(Role::Read).expect("set_role(Read)");
    assert_eq!(database.current_role(), Role::Read, "el rol debe ser Read");

    // Escrituras directas denegadas.
    assert_permission_denied(database.insert(TABLE, scalars(9, "nuevo")), "Read insert");
    let record = vector_record(RecordId::new(), scalars(8, "otro"), &QUERY_VECTOR);
    assert_permission_denied(database.insert_record(TABLE, record), "Read insert_record");

    // DML vía `execute` denegado.
    assert_permission_denied(
        database.execute("UPDATE items SET name = 'z' WHERE id = 1"),
        "Read UPDATE",
    );
    assert_permission_denied(
        database.execute("DELETE FROM items WHERE id = 1"),
        "Read DELETE",
    );

    // `put_blob` es escritura: denegado.
    assert_permission_denied(database.put_blob(BLOB_PAYLOAD), "Read put_blob");

    // Cambio de rol también es administración: denegado y el rol no cambia.
    assert_permission_denied(database.set_role(Role::Admin), "Read set_role");
    assert_eq!(
        database.current_role(),
        Role::Read,
        "una denegación no debe cambiar el rol"
    );

    // `SELECT` sí funciona y los datos permanecen intactos (sin efectos).
    let rows = database
        .execute("SELECT * FROM items")
        .expect("SELECT con rol Read");
    assert_eq!(
        rows.len(),
        SEEDED_SCALAR_ROWS,
        "las escrituras denegadas no deben crear ni borrar filas"
    );
    assert_eq!(
        name_of(&rows, 1),
        Some("origen".to_string()),
        "el UPDATE denegado no debe alterar la fila"
    );
}

/// AC-0066-02 — con rol `Write` la administración se deniega; escribir/leer sí.
#[test]
// @spec AC-0066-02
fn test_ac_0066_02_write_role_cannot_administer() {
    let (_dir, mut database) = open_test_db("ac0066_02");
    create_items(&mut database, TABLE);
    seed_vector_rows(&mut database);

    database.set_role(Role::Write).expect("set_role(Write)");
    assert_eq!(
        database.current_role(),
        Role::Write,
        "el rol debe ser Write"
    );

    // Permitido: escribir, leer, KNN y subir blobs.
    database
        .insert(TABLE, scalars(10, "write"))
        .expect("insert permitido en Write");
    let rows = database
        .execute("SELECT * FROM items")
        .expect("SELECT permitido en Write");
    assert_eq!(rows.len(), SEEDED_VECTOR_ROWS + 1);
    let knn = database
        .execute(&knn_query())
        .expect("KNN permitido en Write");
    assert_eq!(knn.len(), KNN_K, "el KNN top-k debe devolver k filas");
    database
        .put_blob(BLOB_PAYLOAD)
        .expect("put_blob permitido en Write");

    // Denegado: operaciones de administración reservadas a Admin.
    assert_permission_denied(
        database.register_model("modelo", VECTOR_DIM, Metric::L2, 1),
        "Write register_model",
    );
    assert_permission_denied(database.set_role(Role::Admin), "Write set_role");
    assert_permission_denied(database.set_audit(true), "Write set_audit");
    assert_permission_denied(database.clear_audit(), "Write clear_audit");
    assert_permission_denied(database.gc_blobs(), "Write gc_blobs");

    // Las denegaciones no alteraron el rol.
    assert_eq!(
        database.current_role(),
        Role::Write,
        "las denegaciones no deben cambiar el rol"
    );
}

/// AC-0066-03 — el rol por defecto es `Admin`: batería completa sin regresión.
#[test]
// @spec AC-0066-03
fn test_ac_0066_03_admin_role_full_access() {
    let (_dir, mut database) = open_test_db("ac0066_03");
    assert_eq!(
        database.current_role(),
        Role::Admin,
        "el rol por defecto de una base nueva debe ser Admin"
    );

    create_items(&mut database, TABLE);
    seed_vector_rows(&mut database);
    database
        .insert(TABLE, scalars(99, "admin"))
        .expect("insert con Admin");

    let rows = database
        .execute("SELECT * FROM items")
        .expect("SELECT con Admin");
    assert_eq!(rows.len(), SEEDED_VECTOR_ROWS + 1);

    let knn = database.execute(&knn_query()).expect("KNN con Admin");
    assert_eq!(knn.len(), KNN_K, "el KNN debe funcionar sin regresión");

    database
        .put_blob(BLOB_PAYLOAD)
        .expect("put_blob con Admin no debe fallar");
}

/// AC-0066-04 — con audit ON cada operación emite una entrada, en orden.
#[test]
// @spec AC-0066-04
fn test_ac_0066_04_audit_records_operations() {
    let (_dir, mut database) = open_test_db("ac0066_04");
    create_items(&mut database, TABLE);
    database.set_audit(true).expect("set_audit(true)");
    let initial = database.audit_trail();

    database
        .insert(TABLE, scalars(1, "uno"))
        .expect("insert auditado");
    let after_insert = database.audit_trail();

    database
        .execute("SELECT * FROM items")
        .expect("execute auditado");
    let after_select = database.audit_trail();

    database.put_blob(BLOB_PAYLOAD).expect("put_blob auditado");
    let after_put = database.audit_trail();

    // Cada operación añade al menos una entrada nueva.
    assert!(
        after_insert.len() > initial.len(),
        "insert debe registrar una entrada de audit"
    );
    assert!(
        after_select.len() > after_insert.len(),
        "execute debe registrar una entrada de audit"
    );
    assert!(
        after_put.len() > after_select.len(),
        "put_blob debe registrar una entrada de audit"
    );
    assert!(
        after_put.len() >= AUDIT_MIN_ENTRIES,
        "la batería debe registrar al menos {AUDIT_MIN_ENTRIES} entradas"
    );

    // Las entradas previas permanecen como prefijo (orden de ejecución).
    assert_trail_prefix(&initial, &after_insert, "inicial/insert");
    assert_trail_prefix(&after_insert, &after_select, "insert/execute");
    assert_trail_prefix(&after_select, &after_put, "execute/put_blob");

    // Acción y timestamp bien formados, con nanos no decrecientes.
    assert_entries_well_formed(&after_put);
}

/// AC-0066-05 — audit OFF por defecto; `clear_audit` vacía pero no deshabilita.
#[test]
// @spec AC-0066-05
fn test_ac_0066_05_audit_off_by_default_and_clearable() {
    let (_dir, mut database) = open_test_db("ac0066_05");
    create_items(&mut database, TABLE);

    // OFF por defecto: la batería no captura nada (cero overhead de captura).
    database
        .insert(TABLE, scalars(1, "uno"))
        .expect("insert sin audit");
    database
        .execute("SELECT * FROM items")
        .expect("execute sin audit");
    database.put_blob(BLOB_PAYLOAD).expect("put_blob sin audit");
    assert!(
        database.audit_trail().is_empty(),
        "sin habilitar el audit el trail debe estar vacío"
    );

    // ON: a partir de aquí sí se captura.
    database.set_audit(true).expect("set_audit(true)");
    database
        .insert(TABLE, scalars(2, "dos"))
        .expect("insert con audit");
    assert!(
        !database.audit_trail().is_empty(),
        "con el audit habilitado debe haber entradas"
    );

    // `clear_audit` vacía el trail pero no deshabilita la captura.
    database.clear_audit().expect("clear_audit");
    assert!(
        database.audit_trail().is_empty(),
        "clear_audit debe vaciar el trail"
    );
    database
        .insert(TABLE, scalars(3, "tres"))
        .expect("insert tras clear_audit");
    assert!(
        !database.audit_trail().is_empty(),
        "clear_audit no debe deshabilitar el audit"
    );
}
