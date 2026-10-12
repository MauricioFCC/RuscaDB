---
id: SPEC-0067
feature: blob_pipeline
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-multimodal]
  out_of_scope: [procesamiento async en thread real, thumbnails/extraccion real, persistencia de resultados]
fr:
  - { id: FR-0067-01, desc: "cola FIFO de trabajos de blob (blob_pipeline_enqueue -> JobId) con procesado sincrono por lotes (blob_pipeline_drain)" }
  - { id: FR-0067-02, desc: "paso analyze por defecto: (hash, size, media_type, chunk_count con BLOB_CHUNK_SIZE=4096) sin mutar el blob ni su refcount" }
nf:
  - { id: NF-0067-01, desc: "el pipeline NO altera el blob original ni la referencia: put/get posteriores devuelven lo mismo" }
  - { id: NF-0067-02, desc: "errores por trabajo (blob inexistente) no rompen la cola; filas deterministas FIFO; sin panics" }
acceptance_criteria:
  - id: AC-0067-01
    given: dos blobs encolados (texto y binario)
    when: se ejecuta blob_pipeline_drain
    then: resultados con (hash, size, media_type) correctos, en orden FIFO y la cola queda vacia
    test: test_ac_0067_01_drain_processes_fifo_and_empties
  - id: AC-0067-02
    given: un blob encolado sin drain
    when: se consulta blob_pipeline_status
    then: pending == 1 y processed == 0; tras drain ambos se actualizan
    test: test_ac_0067_02_status_tracks_queue
  - id: AC-0067-03
    given: un blob mayor que BLOB_CHUNK_SIZE
    when: se procesa con drain
    then: chunk_count == ceil(size / chunk) y cada chunk es accesible por get_range dentro de limites
    test: test_ac_0067_03_chunking_metrics_computed
  - id: AC-0067-04
    given: un blob encolado y procesado
    when: se compara get_blob antes y despues del drain y el refcount del store
    then: bytes identicos y refcount sin cambios (no-op de fondo)
    test: test_ac_0067_04_pipeline_does_not_mutate_blob
  - id: AC-0067-05
    given: un JobId encolado cuyo hash no existe en el store
    when: se ejecuta blob_pipeline_drain
    then: resultado con error accionable para ese trabajo, la cola continua y sin panics
    test: test_ac_0067_05_unknown_blob_reports_error
exit_criteria:
  - cargo test -p ruscadb --test blob_pipeline -- test_ac_0067
  - cargo test -p ruscadb  (suite previa verde)
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - eliminar el modulo blob_pipeline; el store queda exactamente como estaba
sandbox:
  - cargo test -p ruscadb --test blob_pipeline
---

# SPEC-0067 — Pipeline de blobs

## Contexto

Línea futura del roadmap: **pipeline de blobs**. El blob store CAS ya hace
dedup + refcount; el pipeline añade una cola de procesado (síncrona por lotes,
sin thread: KISS y determinista) con un paso `analyze` que deriva métricas
(size/media_type/chunk_count) sin tocar el contenido ni las referencias. Es la
base para transformaciones reales futuras (thumbnails, extracción, reparto por
chunks) que quedan fuera de alcance.

## Superficie de API

```rust
pub struct BlobJobResult { pub job: u64, pub hash: BlobHash, pub size: usize,
    pub media_type: String, pub chunk_count: usize, pub error: Option<String> }
pub struct PipelineStatus { pub pending: usize, pub processed: usize }
impl Database {
    pub fn blob_pipeline_enqueue(&mut self, hash: &BlobHash) -> Result<u64, RuscaError>;
    pub fn blob_pipeline_drain(&mut self) -> Vec<BlobJobResult>;
    pub fn blob_pipeline_status(&self) -> PipelineStatus;
}
```

- `BLOB_CHUNK_SIZE = 4096` (const con nombre) y `chunk_count = size.div_ceil(CHUNK)`.
- El trabajo guarda el `size` para `get_range` de verificación (no muta el store).

## Criterios de aceptación

- **AC-0067-01/02** — FIFO, estado de la cola.
- **AC-0067-03/04** — métricas de chunking y no-mutación.
- **AC-0067-05** — error por trabajo sin romper la cola.

## Trazabilidad

Tests `test_ac_0067_<nn>_*`; verificado por `cargo xtask trace`/`contract`.