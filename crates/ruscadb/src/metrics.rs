//! Metricas estructuradas por base (SPEC-0188, P2.5).

use crate::database::Database;

/// Snapshot monotono de metricas de la base.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Metrics {
    /// Queries ejecutadas.
    pub queries: u64,
    /// Filas devueltas acumuladas.
    pub rows_returned: u64,
    /// Hits del cache semantico.
    pub cache_hits: u64,
    /// Misses del cache semantico.
    pub cache_misses: u64,
    /// Bytes del WAL en disco.
    pub wal_bytes: u64,
    /// Segmentos WAL presentes.
    pub segments: u64,
}

impl Database {
    /// Devuelve un snapshot de metricas (sin bloquear red/IO externa).
    ///
    /// Returns:
    ///     [`Metrics`] con contadores y tamano del WAL.
    pub fn metrics(&self) -> Metrics {
        let wal_bytes = std::fs::metadata(self.wal_path())
            .map(|meta| meta.len())
            .unwrap_or(0);
        Metrics {
            queries: self.query_count,
            rows_returned: self.rows_returned,
            cache_hits: self.semantic_cache.hits(),
            cache_misses: self.semantic_cache.misses(),
            wal_bytes,
            segments: u64::from(wal_bytes > 0),
        }
    }
}
