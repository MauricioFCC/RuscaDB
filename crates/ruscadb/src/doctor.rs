//! Diagnostico de integridad y estado (SPEC-0188, P2.1).

use ruscadb_core::RuscaError;

use crate::database::Database;
use crate::features::FEATURES;

/// Reporte de salud de una base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    /// Version de esquema del manifiesto.
    pub schema_version: u32,
    /// Integridad global (WAL + heap + vector).
    pub integrity_ok: bool,
    /// WAL decodifica con CRC32C.
    pub wal_ok: bool,
    /// Heap escanea con slots coherentes.
    pub heap_ok: bool,
    /// Indice vectorial cubre las filas vivas.
    pub vector_ok: bool,
    /// Ultima transaccion asignada.
    pub last_tx_id: u64,
    /// Features compiladas en este binario.
    pub features: Vec<&'static str>,
    /// Detalles accionables de fallos (vacio si todo va bien).
    pub details: Vec<String>,
}

impl Database {
    /// Ejecuta el diagnostico de integridad y estado.
    ///
    /// Returns:
    ///     [`DoctorReport`] listo para serializar a JSON.
    ///
    /// Errors:
    ///     Las de [`Database::integrity_check`].
    pub fn doctor(&mut self) -> Result<DoctorReport, RuscaError> {
        let report = self.integrity_check()?;
        Ok(DoctorReport {
            schema_version: self.manifest().schema_version,
            integrity_ok: report.wal_ok && report.heap_ok && report.vector_ok,
            wal_ok: report.wal_ok,
            heap_ok: report.heap_ok,
            vector_ok: report.vector_ok,
            last_tx_id: self.last_tx_id(),
            features: FEATURES.to_vec(),
            details: report.details,
        })
    }
}
