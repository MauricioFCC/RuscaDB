//! Errores tipados del dominio de RuscaDB.

use thiserror::Error;

/// Error unificado del dominio y de los adapters de RuscaDB.
///
/// Es `Send + Sync + 'static` para poder cruzar la frontera FFI y viajar por
/// hilos. Cada variante lleva contexto accionable (WHAT + WHY + WHERE).
#[derive(Debug, Error)]
pub enum RuscaError {
    /// El identificador de registro no es un ULID válido.
    #[error("identificador inválido: {0}")]
    InvalidId(String),

    /// La dimensión declarada no coincide con la del vector.
    #[error("dimensión de embedding inconsistente: esperado {expected}, recibido {actual}")]
    DimensionMismatch {
        /// Dimensión declarada en la metadata del modelo.
        expected: usize,
        /// Dimensión observada en el vector de valores.
        actual: usize,
    },

    /// Error de entrada/salida en disco.
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),

    /// El WAL contiene bytes que no son un frame válido.
    #[error("WAL corrupto: {0}")]
    WalCorrupt(String),

    /// El manifiesto de la base no se pudo leer o validar.
    #[error("manifiesto corrupto: {0}")]
    CorruptManifest(String),

    /// El proceso de recuperación del WAL falló.
    #[error("recovery de WAL falló: {0}")]
    WalRecoveryFailed(String),
}
