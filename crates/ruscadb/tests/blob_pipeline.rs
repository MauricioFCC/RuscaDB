//! Pipeline de blobs en la fachada (SPEC-0067).
//!
//! Tests RED del contrato [`Database::blob_pipeline_enqueue`] /
//! [`Database::blob_pipeline_drain`] / [`Database::blob_pipeline_status`]: una
//! cola FIFO sincrónica (sin threads) con un paso `analyze` que deriva
//! `(size, media_type, chunk_count)` sin mutar el blob ni su refcount.
//!
//! Ver `specs/blob_pipeline.md` (SPEC-0067).

// El oráculo de tests puede fallar ruidosamente (`allow-expect-in-tests` del
// workspace no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use std::path::Path;

use ruscadb::{BlobHash, BlobJobResult, Database, DbConfig, PipelineStatus};

/// Capacidad del buffer pool usada por las bases de prueba.
const POOL_CAPACITY: usize = 64;

/// Tamaño de chunk del pipeline (debe coincidir con `BLOB_CHUNK_SIZE` de la impl).
const CHUNK: usize = 4096;

/// Tamaño del blob binario grande: dos chunks completos más un resto.
const BIG_SIZE: usize = 9000;

/// Abre una base de prueba con blob store bajo `dir`.
///
/// Args:
///     dir: Directorio temporal que aloja los datos y los blobs.
///
/// Returns:
///     La base lista con `blob_path` configurado.
fn open_with_blobs(dir: &Path) -> Database {
    let mut config = DbConfig::new(dir.join("pipeline.data"), POOL_CAPACITY);
    config.blob_path = Some(dir.join("pipeline-blobs"));
    Database::open(config).expect("apertura con blob store")
}

/// Devuelve un patrón binario determinista de `size` bytes.
///
/// Args:
///     size: Número de bytes a generar.
///
/// Returns:
///     El vector de bytes `i % 251` para `i` en `0..size`.
fn pattern(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// Calcula los chunks esperados: `ceil(size / CHUNK)`.
///
/// Args:
///     size: Tamaño del blob en bytes.
///
/// Returns:
///     El número de chunks de tamaño `CHUNK` necesarios.
fn expected_chunks(size: usize) -> usize {
    size.div_ceil(CHUNK)
}

/// Construye un [`BlobHash`] canónico (64 hex) que no existe en el store.
///
/// Returns:
///     Un hash con forma válida pero sin fichero asociado.
fn missing_hash() -> BlobHash {
    BlobHash("deadbeef".repeat(8))
}

/// AC-0067-01 — el drain procesa en FIFO y vacía la cola.
///
/// Encola un blob de texto y uno binario de `BIG_SIZE`; verifica JobIds 1,2,
/// el orden FIFO, los `(hash, size, chunk_count)` exactos, `media_type` no
/// vacía y el estado final `pending == 0` / `processed == 2`.
#[test] // @spec AC-0067-01
fn test_ac_0067_01_drain_processes_fifo_and_empties() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let mut database = open_with_blobs(dir.path());

    let text = b"hola mundo";
    let big = pattern(BIG_SIZE);
    let text_hash = database.put_blob(text).expect("put_blob texto");
    let big_hash = database.put_blob(&big).expect("put_blob binario");

    let first = database
        .blob_pipeline_enqueue(&text_hash)
        .expect("enqueue texto");
    let second = database
        .blob_pipeline_enqueue(&big_hash)
        .expect("enqueue binario");
    assert_eq!(first, 1, "el primer JobId es 1");
    assert_eq!(second, 2, "el segundo JobId es 2");

    let results: Vec<BlobJobResult> = database.blob_pipeline_drain();
    assert_eq!(results.len(), 2, "drena los dos trabajos");

    // FIFO: el orden de salida respeta el de encolado.
    assert_eq!(results[0].job, first, "el primer resultado es el job 1");
    assert_eq!(results[1].job, second, "el segundo resultado es el job 2");
    assert_eq!(results[0].hash, text_hash, "hash del job 1");
    assert_eq!(results[1].hash, big_hash, "hash del job 2");
    assert_eq!(results[0].size, text.len(), "size exacto del texto");
    assert_eq!(results[1].size, BIG_SIZE, "size exacto del binario");
    assert_eq!(
        results[0].chunk_count,
        expected_chunks(text.len()),
        "chunk_count del texto"
    );
    assert_eq!(
        results[1].chunk_count,
        expected_chunks(BIG_SIZE),
        "chunk_count del binario"
    );
    for result in &results {
        assert!(result.error.is_none(), "sin error en el job {}", result.job);
        assert!(
            !result.media_type.is_empty(),
            "media_type no vacío en el job {}",
            result.job
        );
    }

    let status: PipelineStatus = database.blob_pipeline_status();
    assert_eq!(status.pending, 0, "cola vacía tras el drain");
    assert_eq!(status.processed, 2, "dos trabajos procesados");
}

/// AC-0067-02 — el estado refleja la cola antes y después del drain.
#[test] // @spec AC-0067-02
fn test_ac_0067_02_status_tracks_queue() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let mut database = open_with_blobs(dir.path());
    let hash = database.put_blob(b"trabajo pendiente").expect("put_blob");

    database
        .blob_pipeline_enqueue(&hash)
        .expect("enqueue del trabajo");

    let before: PipelineStatus = database.blob_pipeline_status();
    assert_eq!(before.pending, 1, "un trabajo pendiente sin drain");
    assert_eq!(before.processed, 0, "ninguno procesado aún");

    let drained: Vec<BlobJobResult> = database.blob_pipeline_drain();
    assert_eq!(drained.len(), 1, "drena el único trabajo");

    let after: PipelineStatus = database.blob_pipeline_status();
    assert_eq!(after.pending, 0, "cola vacía tras el drain");
    assert_eq!(after.processed, 1, "un trabajo procesado");
}

/// AC-0067-03 — métricas de chunking sobre un blob mayor que `BLOB_CHUNK_SIZE`.
///
/// `ceil(9000 / 4096) == 3`; además el blob completo sigue siendo recuperable
/// con `get_blob` (integridad del tamaño).
#[test] // @spec AC-0067-03
fn test_ac_0067_03_chunking_metrics_computed() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let mut database = open_with_blobs(dir.path());
    let big = pattern(BIG_SIZE);
    let hash = database.put_blob(&big).expect("put_blob binario");

    database
        .blob_pipeline_enqueue(&hash)
        .expect("enqueue binario");
    let results: Vec<BlobJobResult> = database.blob_pipeline_drain();
    assert_eq!(results.len(), 1, "un único trabajo");

    assert_eq!(results[0].chunk_count, 3, "ceil(9000/4096) = 3");
    assert_eq!(
        results[0].chunk_count,
        expected_chunks(BIG_SIZE),
        "chunk_count derivado de BLOB_CHUNK_SIZE"
    );
    assert_eq!(results[0].size, BIG_SIZE, "size íntegro");

    let recovered = database.get_blob(&hash).expect("get_blob completo");
    assert_eq!(recovered.len(), BIG_SIZE, "tamaño íntegro recuperable");
    assert_eq!(recovered, big, "bytes íntegros tras el procesado");
}

/// AC-0067-04 — el pipeline no muta el blob ni su referencia (no-op de fondo).
///
/// Compara `get_blob` antes y después del drain: bytes idénticos.
#[test] // @spec AC-0067-04
fn test_ac_0067_04_pipeline_does_not_mutate_blob() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let mut database = open_with_blobs(dir.path());
    let content = b"contenido inmutable 0067";
    let hash = database.put_blob(content).expect("put_blob");

    let before = database.get_blob(&hash).expect("get_blob antes del drain");

    database
        .blob_pipeline_enqueue(&hash)
        .expect("enqueue del trabajo");
    let drained: Vec<BlobJobResult> = database.blob_pipeline_drain();
    assert_eq!(drained.len(), 1, "un trabajo procesado");

    let after = database
        .get_blob(&hash)
        .expect("get_blob después del drain");
    assert_eq!(after, before, "el pipeline no muta los bytes del blob");
    assert_eq!(after, content, "el contenido original permanece íntegro");
}

/// AC-0067-05 — un hash inexistente reporta error sin romper la cola.
#[test] // @spec AC-0067-05
fn test_ac_0067_05_unknown_blob_reports_error() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let mut database = open_with_blobs(dir.path());
    let ghost = missing_hash();

    let job = database
        .blob_pipeline_enqueue(&ghost)
        .expect("encolar un hash desconocido no falla");
    let results: Vec<BlobJobResult> = database.blob_pipeline_drain();

    assert_eq!(results.len(), 1, "un resultado por el trabajo fallido");
    assert_eq!(results[0].job, job, "el resultado conserva el JobId");
    assert_eq!(results[0].hash, ghost, "el resultado conserva el hash");
    let error = results[0]
        .error
        .as_ref()
        .expect("error accionable presente");
    assert!(!error.is_empty(), "mensaje de error no vacío");

    let status: PipelineStatus = database.blob_pipeline_status();
    assert_eq!(
        status.pending, 0,
        "la cola se vacía aunque el trabajo falle"
    );
}
