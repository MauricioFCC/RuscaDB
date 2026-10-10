//! Suite de aceptación del MVP de RuscaDB (SPEC-0031).
//!
//! Flujo end-to-end con la API pública estable: abrir (builder), crear tabla,
//! insertar (fila + lote), crear índice, consultar (`SELECT`/`WHERE`/`MATCH` /
//! `KNN`/`TRAVERSE`/`LIMIT`), borrar, transaccionar, `reap`, manifiesto y
//! reapertura durable; en claro y cifrado, más la existencia de `docs/MVP.md`.

#![forbid(unsafe_code)]
// El oráculo usa `expect` con mensaje (patrón de `executor_integration.rs`:
// el `allow-expect` del workspace no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, Edge, EdgeSet, Embedding, EmbeddingMeta,
    EncryptionConfig, Metric, Record, RecordId, RecordMeta, Row, ScalarMap, ScalarValue, Snapshot,
};

/// Clave fija de pruebas del flujo cifrado (32 B deterministas).
const ENC_KEY: [u8; 32] = [0x2au8; 32];

/// Marcador en claro que jamás debe aparecer en disco cifrado.
const ENC_MARKER: &str = "RUSCADB_MVP_0031_SECRETO_EN_CLARO";

/// Abre una base temporal de pruebas con el builder fluido.
///
/// Args:
///     name: Sufijo del fichero (aísla cada test).
///     pool: Marcos del buffer pool.
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(name: &str, pool: usize) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal para el MVP");
    let path = dir.path().join(format!("{name}.db"));
    let database = Database::builder()
        .data_path(&path)
        .pool_capacity(pool)
        .open()
        .expect("apertura de la base MVP");
    (dir, database)
}

/// Crea la tabla `docs(title TEXT, score FLOAT)` del MVP.
///
/// Args:
///     database: Base abierta donde crear la tabla.
fn create_docs_table(database: &mut Database) {
    database
        .create_table(
            "docs",
            vec![
                ColumnDef {
                    name: "title".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "score".to_string(),
                    col_type: ColumnType::Float,
                },
            ],
        )
        .expect("create_table docs");
}

/// Construye los escalares `(title, score)` de un documento.
///
/// Args:
///     title: Título textual del documento.
///     score: Puntuación asociada.
///
/// Returns:
///     El mapa de escalares listo para insertar.
fn doc_scalars(title: &str, score: f64) -> ScalarMap {
    ScalarMap::from([
        ("title".to_string(), ScalarValue::Text(title.to_string())),
        ("score".to_string(), ScalarValue::Float(score)),
    ])
}

/// Crea un embedding L2 con la dimensión del vector.
///
/// Args:
///     values: Componentes del vector.
///
/// Returns:
///     El embedding con metadata de pruebas.
fn make_embedding(values: &[f64]) -> Embedding {
    let floats: Vec<f32> = values.iter().map(|value| *value as f32).collect();
    Embedding::new(
        floats,
        EmbeddingMeta {
            model_id: "mvp-test".to_string(),
            dim: values.len(),
            metric: Metric::L2,
        },
    )
    .expect("embedding válido para el MVP")
}

/// Construye un documento completo (escalares + vector + aristas).
///
/// Args:
///     title: Título textual del documento.
///     score: Puntuación asociada.
///     vector: Embedding opcional (componentes `f64`).
///     edges: Aristas salientes del documento.
///
/// Returns:
///     El registro listo para `insert_record` / `insert_many`.
fn make_doc(title: &str, score: f64, vector: Option<Vec<f64>>, edges: Vec<Edge>) -> Record {
    Record {
        id: RecordId::new(),
        scalars: doc_scalars(title, score),
        doc: None,
        edges: EdgeSet {
            out: edges,
            incoming: Vec::new(),
        },
        vector: vector.as_deref().map(make_embedding),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Crea una arista saliente hacia otro registro.
///
/// Args:
///     node: Identificador del nodo destino.
///
/// Returns:
///     La arista con etiqueta `link`.
fn edge_to(node: RecordId) -> Edge {
    Edge {
        label: "link".to_string(),
        node,
    }
}

/// Extrae el texto de una columna de una fila.
///
/// Args:
///     row: Fila resultado.
///     column: Columna textual.
///
/// Returns:
///     El contenido en texto.
fn text_of(row: &Row, column: &str) -> String {
    match row.get(column).expect("columna presente en la fila") {
        ScalarValue::Text(value) => value.clone(),
        other => panic!("se esperaba Text en '{column}', se obtuvo {other:?}"),
    }
}

/// Extrae el flotante de una columna de una fila.
///
/// Args:
///     row: Fila resultado.
///     column: Columna numérica.
///
/// Returns:
///     El valor como `f64`.
fn score_of(row: &Row, column: &str) -> f64 {
    match row.get(column).expect("columna presente en la fila") {
        ScalarValue::Float(value) => *value,
        ScalarValue::Int(value) => *value as f64,
        other => panic!("se esperaba Float en '{column}', se obtuvo {other:?}"),
    }
}

/// Indica si `haystack` contiene `needle` (con guarda de longitud).
///
/// Args:
///     haystack: Bytes del fichero.
///     needle: Marcador buscado.
///
/// Returns:
///     `true` si el marcador aparece en claro.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Siembra el happy path: una fila + un lote de tres (AC-0031-01).
///
/// Args:
///     database: Base con la tabla `docs` ya creada.
///
/// Returns:
///     Los cuatro identificadores insertados, en orden.
fn seed_happy_path(database: &mut Database) -> Vec<RecordId> {
    let first = database
        .insert("docs", doc_scalars("mvp uno", 1.5))
        .expect("insert de la primera fila");
    let batch = vec![
        make_doc("mvp dos", 2.5, None, Vec::new()),
        make_doc("mvp tres", 0.5, None, Vec::new()),
        make_doc("mvp cuatro", 3.5, None, Vec::new()),
    ];
    let mut ids = vec![first];
    ids.extend(
        database
            .insert_many("docs", batch)
            .expect("insert_many del lote MVP"),
    );
    ids
}

/// Verifica `SELECT *`, `WHERE` + `LIMIT` y `execute_at` con snapshot.
///
/// Args:
///     database: Base con las cuatro filas del happy path.
fn assert_happy_queries(database: &mut Database) {
    let all = database
        .execute("SELECT * FROM docs")
        .expect("SELECT * del MVP");
    assert_eq!(all.len(), 4, "una fila + lote de tres");
    let top = database
        .execute("SELECT * FROM docs WHERE score > 1.0 LIMIT 2")
        .expect("WHERE con LIMIT");
    assert_eq!(top.len(), 2, "LIMIT acota el resultado");
    assert!(top.iter().all(|row| score_of(row, "score") > 1.0));
    let snapshot: Snapshot = database.snapshot();
    let via_snapshot = database
        .execute_at("SELECT * FROM docs", &snapshot)
        .expect("execute_at con snapshot");
    assert_eq!(via_snapshot, all, "el snapshot ve lo confirmado");
}

/// Verifica borrado lógico + `reap` + `get_record` (AC-0031-01).
///
/// Args:
///     database: Base con las filas del happy path.
///     ids: Identificadores sembrados (`ids[0]` se borra).
fn assert_delete_and_reap(database: &mut Database, ids: &[RecordId]) {
    let doomed = &ids[0];
    assert!(
        database.delete("docs", doomed).expect("delete del MVP"),
        "la fila existía viva"
    );
    assert!(
        !database.delete("docs", doomed).expect("delete repetido"),
        "el borrado es idempotente"
    );
    let visible = database
        .execute("SELECT * FROM docs")
        .expect("SELECT tras delete");
    assert_eq!(visible.len(), 3, "la fila borrada deja de verse");
    assert_eq!(database.reap().expect("reap del MVP"), 1);
    assert!(
        database
            .get_record("docs", doomed)
            .expect("get_record")
            .is_none(),
        "la fila purgada ya no se recupera"
    );
    assert!(
        database
            .get_record("docs", &ids[1])
            .expect("get_record")
            .is_some(),
        "la fila viva sigue recuperable"
    );
}

/// Verifica transacciones, rollback, manifiesto y catálogo (AC-0031-01).
///
/// Args:
///     database: Base con tres filas visibles.
fn assert_tx_and_meta(database: &mut Database) {
    let pending = database.begin();
    assert_eq!(database.active_tx(), Some(pending), "la tx queda en vuelo");
    database.rollback().expect("rollback sin cambios");
    assert_eq!(database.active_tx(), None, "el rollback limpia la tx");
    let steady = database
        .execute("SELECT * FROM docs")
        .expect("SELECT tras rollback");
    assert_eq!(steady.len(), 3, "sin cambios no hay efectos");
    let _live = database.begin();
    database
        .insert("docs", doc_scalars("mvp tx", 4.0))
        .expect("insert en tx");
    database.commit().expect("commit del MVP");
    assert_eq!(database.active_tx(), None, "el commit limpia la tx");
    let rows = database
        .execute("SELECT * FROM docs")
        .expect("SELECT tras commit");
    assert_eq!(rows.len(), 4, "la fila transaccionada es visible");
}

/// Verifica manifiesto, catálogo y LSN tras el flujo (AC-0031-01).
///
/// Args:
///     database: Base con el flujo completo aplicado.
fn assert_manifest_and_catalog(database: &mut Database) {
    assert!(database.last_lsn() > 0, "hubo commits WAL-first");
    assert_eq!(database.manifest().checkpoint_lsn, database.last_lsn());
    assert!(database.manifest().epoch >= 1, "cada commit publica época");
    let tables = database.tables().expect("tables del MVP");
    assert!(
        tables.contains(&"docs".to_string()),
        "docs sigue catalogada"
    );
    assert!(database.catalog().expect("catalog").contains("docs"));
}

/// AC-0031-01 — flujo MVP completo en claro con durabilidad total.
#[test]
// @spec AC-0031-01
fn test_ac_0031_01_mvp_happy_path() {
    let (dir, mut database) = open_test_db("mvp_happy", 64);
    create_docs_table(&mut database);
    let ids = seed_happy_path(&mut database);
    database
        .create_index("docs", "title")
        .expect("create_index title");
    assert_happy_queries(&mut database);
    assert_delete_and_reap(&mut database, &ids);
    assert_tx_and_meta(&mut database);
    assert_manifest_and_catalog(&mut database);
    database.close().expect("close del MVP");
    let path = dir.path().join("mvp_happy.db");
    let mut reopened = Database::open(DbConfig::new(&path, 64)).expect("reapertura del MVP");
    let rows = reopened
        .execute("SELECT * FROM docs")
        .expect("SELECT tras reopen");
    assert_eq!(rows.len(), 4, "persisten las filas vivas");
    let indexed = reopened
        .execute("SELECT * FROM docs WHERE title = 'mvp tx'")
        .expect("WHERE con índice tras reopen");
    assert_eq!(indexed.len(), 1, "el índice sobrevive a reopen");
    assert!(
        reopened
            .get_record("docs", &ids[1])
            .expect("get_record")
            .is_some(),
        "la fila viva sigue recuperable"
    );
    assert!(
        reopened
            .get_record("docs", &ids[0])
            .expect("get_record")
            .is_none(),
        "la fila purgada no resucita"
    );
    reopened.close().expect("close tras reopen");
}

/// Siembra cuatro documentos multimodales (vector + aristas + texto).
///
/// Cadena `norte -> centro -> sur`; `isla` queda aislada.
///
/// Args:
///     database: Base con la tabla `docs` ya creada.
fn seed_multimodel(database: &mut Database) {
    let norte = RecordId::new();
    let centro = RecordId::new();
    let sur = RecordId::new();
    let isla = RecordId::new();
    let seeds = vec![
        (
            norte,
            "gato duerme en el sofa",
            1.0,
            vec![0.0, 0.0, 0.0],
            vec![edge_to(centro)],
        ),
        (
            centro,
            "gato gato caza",
            2.0,
            vec![0.1, 0.1, 0.1],
            vec![edge_to(sur)],
        ),
        (
            sur,
            "perro corre en el parque",
            3.0,
            vec![9.0, 9.0, 9.0],
            Vec::new(),
        ),
        (
            isla,
            "casa azul junto al lago",
            4.0,
            vec![5.0, 5.0, 5.0],
            Vec::new(),
        ),
    ];
    for (id, title, score, vector, edges) in seeds {
        let mut record = make_doc(title, score, Some(vector), edges);
        record.id = id;
        let stored = database
            .insert_record("docs", record)
            .expect("insert_record multimodal");
        assert_eq!(stored, id, "insert_record conserva el id");
    }
}

/// AC-0031-02 — multimodal: KNN + TRAVERSE + MATCH sobre los mismos docs.
#[test]
// @spec AC-0031-02
fn test_ac_0031_02_mvp_multimodel() {
    let (_dir, mut database) = open_test_db("mvp_multi", 64);
    create_docs_table(&mut database);
    seed_multimodel(&mut database);
    let nearest = database
        .execute("SELECT * FROM docs KNN embedding <|2|> [0.1, 0.2, 0.3]")
        .expect("KNN multimodal");
    assert_eq!(nearest.len(), 2, "los dos vecinos más cercanos");
    assert_eq!(text_of(&nearest[0], "title"), "gato gato caza");
    assert_eq!(text_of(&nearest[1], "title"), "gato duerme en el sofa");
    let reached = database
        .execute("SELECT * FROM docs TRAVERSE edges DEPTH 2")
        .expect("TRAVERSE multimodal");
    let names: Vec<String> = reached.iter().map(|row| text_of(row, "title")).collect();
    assert_eq!(
        names,
        [
            "gato duerme en el sofa",
            "gato gato caza",
            "perro corre en el parque"
        ],
        "la isla aislada queda fuera"
    );
    let matched = database
        .execute("SELECT * FROM docs WHERE MATCH(title, 'gato')")
        .expect("MATCH multimodal");
    assert_eq!(matched.len(), 2, "solo los documentos con 'gato'");
    assert_eq!(text_of(&matched[0], "title"), "gato gato caza");
}

/// Configuración cifrada de pruebas para una ruta.
///
/// Args:
///     path: Ruta del fichero `.data`.
///
/// Returns:
///     `DbConfig` con pool amplio y la clave fija.
fn encrypted_config(path: &std::path::Path) -> DbConfig {
    let mut config = DbConfig::new(path, 128);
    config.encryption = Some(EncryptionConfig::new(ENC_KEY));
    config
}

/// Siembra el flujo cifrado y devuelve el id del marcador.
///
/// Args:
///     database: Base cifrada abierta.
///
/// Returns:
///     El identificador de la fila con el marcador en claro.
fn seed_encrypted(database: &mut Database) -> RecordId {
    create_docs_table(database);
    let marker = database
        .insert("docs", doc_scalars(ENC_MARKER, 7.5))
        .expect("insert del marcador cifrado");
    database
        .insert_many("docs", vec![make_doc("cifrado dos", 1.0, None, Vec::new())])
        .expect("lote cifrado");
    database
        .create_index("docs", "title")
        .expect("create_index cifrado");
    let rows = database
        .execute("SELECT * FROM docs")
        .expect("SELECT cifrado");
    assert_eq!(rows.len(), 2, "marcador + lote");
    marker
}

/// Reabre la base cifrada con la clave fija.
///
/// Args:
///     path: Ruta del fichero `.data`.
///
/// Returns:
///     La base lista para verificar.
fn reopen_encrypted(path: &std::path::Path) -> Database {
    Database::open(encrypted_config(path)).expect("reapertura cifrada")
}

/// Verifica que el marcador en claro no aparece en `.data` ni `.wal`.
///
/// Args:
///     path: Ruta del fichero `.data` de la base cifrada.
fn assert_no_plaintext(path: &std::path::Path) {
    let data = std::fs::read(path).expect("lee el fichero .data");
    let wal = std::fs::read(path.with_extension("wal")).expect("lee el fichero .wal");
    assert!(!wal.is_empty(), "el WAL cifrado contiene frames");
    assert!(
        !contains_bytes(&data, ENC_MARKER.as_bytes()),
        ".data sin claro"
    );
    assert!(
        !contains_bytes(&wal, ENC_MARKER.as_bytes()),
        ".wal sin claro"
    );
}

/// AC-0031-03 — flujo MVP cifrado sin claro en disco.
#[test]
// @spec AC-0031-03
fn test_ac_0031_03_mvp_encrypted() {
    let dir = tempfile::tempdir().expect("directorio temporal cifrado");
    let path = dir.path().join("mvp_enc.db");
    {
        let mut database = Database::builder()
            .data_path(&path)
            .pool_capacity(128)
            .encryption(Some(EncryptionConfig::new(ENC_KEY)))
            .open()
            .expect("apertura cifrada del MVP");
        let marker = seed_encrypted(&mut database);
        assert!(
            database.delete("docs", &marker).expect("delete cifrado"),
            "el marcador existía vivo"
        );
        let active = database.begin();
        assert_eq!(database.active_tx(), Some(active));
        database.commit().expect("commit cifrado");
        database.close().expect("close cifrado");
    }
    assert_no_plaintext(&path);
    let mut reopened = reopen_encrypted(&path);
    let rows = reopened
        .execute("SELECT * FROM docs")
        .expect("SELECT tras reopen");
    assert_eq!(rows.len(), 1, "solo sobrevive el lote");
    assert_eq!(text_of(&rows[0], "title"), "cifrado dos");
    reopened.close().expect("close tras reopen");
}

/// AC-0031-04 — `docs/MVP.md` existe y define el MVP.
#[test]
// @spec AC-0031-04
fn test_ac_0031_04_mvp_doc_exists() {
    let doc: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/MVP.md");
    assert!(doc.is_file(), "docs/MVP.md debe existir en {doc:?}");
    let text = std::fs::read_to_string(&doc).expect("lee docs/MVP.md");
    for section in ["## Capacidades", "## Límites", "## API estable"] {
        assert!(text.contains(section), "falta la sección {section}");
    }
}
