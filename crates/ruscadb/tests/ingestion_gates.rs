//! Tests de aceptación de los gates de ingesta + registry persistible
//! (SPEC-0064).
//!
//! Cubre los criterios AC-0064-01..05: rechazo de registros que exceden
//! `max_record_bytes` (`RuscaError::ResourceLimit`), rechazo de embeddings de
//! modelos no allowlisted con `require_registry`, persistencia del
//! `ModelRegistry` en `<data>.models.json` entre reopens, y cero regresión con
//! la política por defecto (abierta: `0`/`false`).
//!
//! Estos tests son RED por diseño: la API de SPEC-0064 aún no existe en la
//! fachada `ruscadb`, por lo que la compilación falla al resolver los símbolos
//! `IngestionPolicy`, `Database::set_ingestion_policy` y
//! `Database::ingestion_policy`. Cuando la implementación aterrice, además
//! fallan las aserciones de comportamiento (registry persistente, gates).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, IngestionPolicy,
    Metric, Record, RecordId, RecordMeta, RuscaError, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 64;

/// Tope nominal de bytes por registro usado para ejercer el gate de tamaño.
const MAX_RECORD_BYTES: usize = 1024;

/// Relleno del documento grande: supera con holgura el tope del gate (1024) y
/// cabe en una página del heap (PAGE_SIZE = 4096 sin overflow pages, fuera de
/// alcance de SPEC-0064): postcard ≈ padding + ~40 B de overhead.
const LARGE_DOC_PADDING: usize = 2048;

/// Dimensión del modelo allowlisted de prueba.
const DIM_REGISTERED: usize = 3;

/// Dimensión del embedding ajeno (rompe la allowlist por modelo y por dim).
const DIM_FOREIGN: usize = 5;

/// Identificador del modelo allowlisted.
const MODEL_ID: &str = "m1";

/// Identificador de un modelo NO allowlisted.
const FOREIGN_MODEL_ID: &str = "m2";

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

/// Crea un embedding de la dimensión pedida, con la métrica indicada.
///
/// Args:
///     model_id: Identificador de procedencia del vector.
///     dim: Dimensión (número de componentes).
///     metric: Métrica de distancia declarada.
///
/// Returns:
///     El embedding construido (componentes a `0.0`).
fn embedding(model_id: &str, dim: usize, metric: Metric) -> Embedding {
    let values: Vec<f32> = vec![0.0; dim];
    Embedding::new(
        values,
        EmbeddingMeta {
            model_id: model_id.to_string(),
            dim,
            metric,
        },
    )
    .expect("embedding válido")
}

/// Construye un documento JSON grande (supera con holgura `MAX_RECORD_BYTES`).
///
/// Returns:
///     El texto JSON `{"payload":"<LARGE_DOC_PADDING caracteres>"}`.
fn large_doc_json() -> String {
    format!(r#"{{"payload":"{}"}}"#, "A".repeat(LARGE_DOC_PADDING))
}

/// Construye un [`Record`] con escalares, documento opcional y vector opcional.
///
/// Args:
///     id: Identificador del registro.
///     scalars: Escalares del registro.
///     doc_json: Documento JSON como texto (`None` = sin documento); se parsea
///         al tipo `serde_json::Value` del campo `doc` sin nombrar el crate.
///     vector: Embedding opcional con su metadata de modelo.
///
/// Returns:
///     El registro listo para `insert_record`.
fn record(
    id: RecordId,
    scalars: ScalarMap,
    doc_json: Option<&str>,
    vector: Option<Embedding>,
) -> Record {
    Record {
        id,
        scalars,
        doc: doc_json.map(|text| text.parse().expect("documento JSON válido")),
        edges: EdgeSet::default(),
        vector,
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Mide el tamaño serializado del [`Record`] tal como lo hará el gate.
///
/// Proxy de tamaño acordado con la spec (SPEC-0064): la implementación
/// serializa el `Record` con `postcard` y compara la longitud contra
/// `max_record_bytes`; este helper usa la MISMA codificación (`postcard`) para
/// que el fixture sea representativo de lo que el gate medirá en producción.
///
/// Args:
///     record: Registro a medir.
///
/// Returns:
///     La longitud en bytes de `postcard(Record)`.
fn postcard_len(record: &Record) -> usize {
    postcard::to_allocvec(record)
        .expect("serialización postcard del registro")
        .len()
}

/// AC-0064-01 — un registro cuyo payload serializado supera el tope se rechaza
/// con `ResourceLimit` accionable y la tabla queda sin la fila.
#[test]
// @spec AC-0064-01
fn test_ac_0064_01_oversized_record_rejected() {
    let (_dir, mut database) = open_test_db("ac0064_01");
    database.set_ingestion_policy(IngestionPolicy {
        max_record_bytes: MAX_RECORD_BYTES,
        require_registry: false,
    });
    assert_eq!(
        database.ingestion_policy().max_record_bytes,
        MAX_RECORD_BYTES,
        "la política fijada debe leerse de vuelta"
    );
    create_items(&mut database, "items");

    let doc = large_doc_json();
    let oversized = record(RecordId::new(), scalars(1, "grande"), Some(&doc), None);
    let size = postcard_len(&oversized);
    assert!(
        size > MAX_RECORD_BYTES,
        "el fixture debe superar el tope para ejercer el gate: {size} <= {MAX_RECORD_BYTES}"
    );

    let error = database
        .insert_record("items", oversized)
        .expect_err("un registro sobredimensionado debe rechazarse");
    assert!(
        matches!(error, RuscaError::ResourceLimit { .. }),
        "se esperaba ResourceLimit, se obtuvo {error:?}"
    );
    assert!(
        error.to_string().contains(&MAX_RECORD_BYTES.to_string()),
        "el error debe citar el tope configurado: {error}"
    );

    let rows = database.execute("SELECT * FROM items").expect("select");
    assert!(
        rows.is_empty(),
        "el rechazo no debe dejar la fila en la tabla: {} filas",
        rows.len()
    );
}

/// AC-0064-02 — con `require_registry` un embedding de modelo ajeno se rechaza
/// con detalle (modelo y dimensiones) y sin insertar.
#[test]
// @spec AC-0064-02
fn test_ac_0064_02_unregistered_model_rejected() {
    let (_dir, mut database) = open_test_db("ac0064_02");
    create_items(&mut database, "items");
    database
        .register_model(MODEL_ID, DIM_REGISTERED, Metric::L2, 1)
        .expect("register_model");
    database.set_ingestion_policy(IngestionPolicy {
        max_record_bytes: 0,
        require_registry: true,
    });
    assert!(
        database.ingestion_policy().require_registry,
        "la política debe quedar exigiendo registro de modelos"
    );

    let foreign = record(
        RecordId::new(),
        scalars(1, "ajeno"),
        None,
        Some(embedding(FOREIGN_MODEL_ID, DIM_FOREIGN, Metric::L2)),
    );
    let error = database
        .insert_record("items", foreign)
        .expect_err("un modelo no registrado debe rechazarse");
    assert!(
        matches!(error, RuscaError::InvalidConfig(_)),
        "se esperaba InvalidConfig, se obtuvo {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains(FOREIGN_MODEL_ID),
        "el error debe citar el modelo obtenido '{FOREIGN_MODEL_ID}': {message}"
    );

    // El modelo sí registrado, con dimensión equivocada, cita modelo y dims.
    let wrong_dim = record(
        RecordId::new(),
        scalars(2, "dim_mala"),
        None,
        Some(embedding(MODEL_ID, DIM_FOREIGN, Metric::L2)),
    );
    let dim_error = database
        .insert_record("items", wrong_dim)
        .expect_err("una dimensión incompatible debe rechazarse");
    let dim_message = dim_error.to_string();
    assert!(
        dim_message.contains(MODEL_ID),
        "el detalle debe citar el modelo '{MODEL_ID}': {dim_message}"
    );
    assert!(
        dim_message.contains(&DIM_REGISTERED.to_string())
            && dim_message.contains(&DIM_FOREIGN.to_string()),
        "el detalle debe citar las dimensiones registrada/recibida: {dim_message}"
    );

    let rows = database.execute("SELECT * FROM items").expect("select");
    assert!(
        rows.is_empty(),
        "ningún rechazo debe dejar filas en la tabla: {} filas",
        rows.len()
    );
}

/// AC-0064-03 — el `ModelRegistry` persiste entre reopens: `validate` sigue
/// pasando para el modelo registrado y rechaza un modelo ajeno.
#[test]
// @spec AC-0064-03
fn test_ac_0064_03_registry_persists_across_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac0064_03.db");

    let mut first = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
    create_items(&mut first, "items");
    first
        .register_model(MODEL_ID, DIM_REGISTERED, Metric::L2, 1)
        .expect("register_model");
    first.set_ingestion_policy(IngestionPolicy {
        max_record_bytes: 0,
        require_registry: true,
    });
    let before = record(
        RecordId::new(),
        scalars(1, "antes"),
        None,
        Some(embedding(MODEL_ID, DIM_REGISTERED, Metric::L2)),
    );
    first
        .insert_record("items", before)
        .expect("modelo registrado antes de cerrar");
    first.close().expect("cierre");

    let mut reopened = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("reapertura");
    reopened.set_ingestion_policy(IngestionPolicy {
        max_record_bytes: 0,
        require_registry: true,
    });

    // El modelo m1 sigue allowlisted tras reabrir: su inserción pasa.
    let after = record(
        RecordId::new(),
        scalars(2, "despues"),
        None,
        Some(embedding(MODEL_ID, DIM_REGISTERED, Metric::L2)),
    );
    reopened
        .insert_record("items", after)
        .expect("el registry debe sobrevivir al reopen");

    // Y la allowlist persistida sigue rechazando un modelo ajeno.
    let foreign = record(
        RecordId::new(),
        scalars(3, "ajeno"),
        None,
        Some(embedding(FOREIGN_MODEL_ID, DIM_FOREIGN, Metric::L2)),
    );
    let error = reopened
        .insert_record("items", foreign)
        .expect_err("la allowlist persistida debe rechazar un modelo ajeno");
    assert!(
        error.to_string().contains(FOREIGN_MODEL_ID),
        "el rechazo debe citar el modelo ajeno '{FOREIGN_MODEL_ID}': {error}"
    );

    let rows = reopened.execute("SELECT * FROM items").expect("select");
    assert_eq!(
        rows.len(),
        2,
        "deben persistir la fila previa y la posterior al reopen"
    );
}

/// AC-0064-04 — sin tocar la política, la base está abierta (cero regresión):
/// entran un registro grande y otro con un modelo no registrado.
#[test]
// @spec AC-0064-04
fn test_ac_0064_04_default_policy_is_open() {
    let (_dir, mut database) = open_test_db("ac0064_04");
    let policy = database.ingestion_policy();
    assert_eq!(
        policy.max_record_bytes, 0,
        "por defecto no debe haber límite de tamaño"
    );
    assert!(
        !policy.require_registry,
        "por defecto no debe exigirse registro de modelos"
    );

    create_items(&mut database, "items");

    let doc = large_doc_json();
    let big = record(RecordId::new(), scalars(1, "grande"), Some(&doc), None);
    let size = postcard_len(&big);
    assert!(
        size > MAX_RECORD_BYTES,
        "el fixture debe superar el tope nominal: {size} <= {MAX_RECORD_BYTES}"
    );
    database
        .insert_record("items", big)
        .expect("la política abierta admite un registro grande");

    let vector = record(
        RecordId::new(),
        scalars(2, "vector_ajeno"),
        None,
        Some(embedding(FOREIGN_MODEL_ID, DIM_FOREIGN, Metric::L2)),
    );
    database
        .insert_record("items", vector)
        .expect("la política abierta admite un modelo no registrado");

    let rows = database.execute("SELECT * FROM items").expect("select");
    assert_eq!(
        rows.len(),
        2,
        "ambos registros deben entrar con la política por defecto"
    );
}

/// AC-0064-05 — la política explícita desactivada (`0`/`false`) no filtra nada:
/// entran un registro grande y uno con modelo no registrado.
#[test]
// @spec AC-0064-05
fn test_ac_0064_05_disabled_policy_allows_all() {
    let (_dir, mut database) = open_test_db("ac0064_05");
    database.set_ingestion_policy(IngestionPolicy {
        max_record_bytes: 0,
        require_registry: false,
    });
    create_items(&mut database, "items");

    let doc = large_doc_json();
    let big = record(RecordId::new(), scalars(1, "grande"), Some(&doc), None);
    database
        .insert_record("items", big)
        .expect("la política desactivada no filtra por tamaño");

    let vector = record(
        RecordId::new(),
        scalars(2, "vector_ajeno"),
        None,
        Some(embedding(FOREIGN_MODEL_ID, DIM_FOREIGN, Metric::L2)),
    );
    database
        .insert_record("items", vector)
        .expect("la política desactivada no filtra por registry");

    let rows = database.execute("SELECT * FROM items").expect("select");
    assert_eq!(
        rows.len(),
        2,
        "la política desactivada debe admitir ambos registros"
    );
}
