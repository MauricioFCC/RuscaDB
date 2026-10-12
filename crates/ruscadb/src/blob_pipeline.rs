//! Pipeline de blobs: cola FIFO + análisis síncrono por lotes (SPEC-0067).
//!
//! Cola en memoria de trabajos de blob con un paso `analyze` que deriva
//! métricas (tamaño, media type, número de chunks) **sin mutar** el contenido
//! ni las referencias del store. El procesado es síncrono (`drain`) para ser
//! determinista y no introducir threads.

use std::collections::VecDeque;

use ruscadb_core::RuscaError;
use ruscadb_multimodal::BlobHash;

use crate::database::Database;

/// Tamaño de chunk del análisis (4 KiB).
pub(crate) const BLOB_CHUNK_SIZE: usize = 4096;

/// Resultado del análisis de un trabajo de blob (SPEC-0067).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobJobResult {
    /// Identificador del trabajo (1-based, orden de encolado).
    pub job: u64,
    /// Hash del blob analizado.
    pub hash: BlobHash,
    /// Tamaño en bytes (0 si el trabajo falló).
    pub size: usize,
    /// Media type inferido (nunca vacío).
    pub media_type: String,
    /// Número de chunks de `BLOB_CHUNK_SIZE` que caben en el blob.
    pub chunk_count: usize,
    /// Error accionable si el blob no existe o no se pudo leer.
    pub error: Option<String>,
}

/// Estado de la cola del pipeline (SPEC-0067).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PipelineStatus {
    /// Trabajos encolados y aún no procesados.
    pub pending: usize,
    /// Trabajos procesados desde la apertura de la base.
    pub processed: usize,
}

/// Estado interno del pipeline (SPEC-0067).
#[derive(Default)]
pub(crate) struct BlobPipeline {
    /// Cola FIFO `(job_id, hash)`.
    pub(crate) queue: VecDeque<(u64, BlobHash)>,
    /// Siguiente identificador de trabajo.
    pub(crate) next_job: u64,
    /// Contador de trabajos procesados.
    pub(crate) processed: usize,
}

impl Database {
    /// Encola un blob para su análisis (FIFO).
    ///
    /// No valida la existencia del blob: un hash ausente se reporta como error
    /// accionable al hacer `drain` (la cola nunca se rompe por un trabajo malo).
    ///
    /// Args:
    ///     hash: Hash del blob a analizar.
    ///
    /// Returns:
    ///     El identificador del trabajo (1-based, creciente).
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si no hay blob store configurado (implícito en
    ///     `get_blob` al procesar); aquí no se produce error.
    pub fn blob_pipeline_enqueue(&mut self, hash: &BlobHash) -> Result<u64, RuscaError> {
        self.blob_pipeline.next_job += 1;
        let job = self.blob_pipeline.next_job;
        self.blob_pipeline.queue.push_back((job, hash.clone()));
        Ok(job)
    }

    /// Procesa la cola completa en orden FIFO y devuelve los resultados.
    ///
    /// Cada trabajo se analiza (`analyze`): tamaño, media type inferido
    /// (UTF-8 → `text/plain`, resto → `application/octet-stream`) y número de
    /// chunks (`ceil(size / BLOB_CHUNK_SIZE)`). Un blob inexistente produce un
    /// resultado con `error` y no interrumpe la cola.
    ///
    /// Returns:
    ///     Un [`BlobJobResult`] por trabajo, en orden de encolado.
    pub fn blob_pipeline_drain(&mut self) -> Vec<BlobJobResult> {
        let mut results = Vec::new();
        while let Some((job, hash)) = self.blob_pipeline.queue.pop_front() {
            let result = match self.get_blob(&hash) {
                Ok(bytes) => BlobJobResult {
                    job,
                    hash,
                    size: bytes.len(),
                    media_type: infer_media_type(&bytes),
                    chunk_count: bytes.len().div_ceil(BLOB_CHUNK_SIZE),
                    error: None,
                },
                Err(error) => BlobJobResult {
                    job,
                    hash,
                    size: 0,
                    media_type: "application/octet-stream".to_string(),
                    chunk_count: 0,
                    error: Some(error.to_string()),
                },
            };
            self.blob_pipeline.processed += 1;
            results.push(result);
        }
        results
    }

    /// Estado actual de la cola del pipeline.
    ///
    /// Returns:
    ///     Pendientes y procesados acumulados.
    pub fn blob_pipeline_status(&self) -> PipelineStatus {
        PipelineStatus {
            pending: self.blob_pipeline.queue.len(),
            processed: self.blob_pipeline.processed,
        }
    }
}

/// Infiere el media type de un contenido sin extensión de fichero.
///
/// Args:
///     bytes: Contenido del blob.
///
/// Returns:
///     `text/plain` si es UTF-8 válido; `application/octet-stream` si no.
fn infer_media_type(bytes: &[u8]) -> String {
    if std::str::from_utf8(bytes).is_ok() {
        "text/plain".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un contenido UTF-8 se infiere como texto.
    #[test]
    fn text_media_type() {
        assert_eq!(infer_media_type(b"hola"), "text/plain");
    }

    /// Un contenido binario se infiere como octet-stream.
    #[test]
    fn binary_media_type() {
        assert_eq!(
            infer_media_type(&[0xFF, 0x00, 0xFE]),
            "application/octet-stream"
        );
    }
}
