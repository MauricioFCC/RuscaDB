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

    /// El buffer pool no tiene marcos desalojables (todas pinneadas o sucias).
    #[error("buffer pool lleno: capacidad {capacity}, sin páginas desalojables")]
    BufferPoolFull {
        /// Capacidad configurada del pool (número de marcos).
        capacity: usize,
    },

    /// El `PageId` solicitado está fuera del rango del archivo.
    #[error("página fuera de rango: id {id}, total {page_count}")]
    PageOutOfRange {
        /// Identificador de página solicitado.
        id: u64,
        /// Número total de páginas del archivo.
        page_count: u64,
    },

    /// Se intentó despinnear una página que no está en el pool.
    #[error("página no presente en el buffer pool: id {id}")]
    PageNotInPool {
        /// Identificador de página.
        id: u64,
    },

    /// Configuración inválida del motor.
    #[error("configuración inválida: {0}")]
    InvalidConfig(String),

    /// La consulta no se pudo analizar sintácticamente.
    #[error("error de sintaxis en posición {position}: {message}")]
    ParseError {
        /// Descripción del error.
        message: String,
        /// Posición (byte) dentro del texto de la consulta.
        position: usize,
    },

    /// La tabla solicitada no existe en el catálogo.
    #[error("tabla no encontrada: '{table}' (no existe en el catálogo; créala con create_table)")]
    TableNotFound {
        /// Nombre de la tabla solicitada.
        table: String,
    },

    /// La columna solicitada no existe en el esquema de la tabla.
    #[error(
        "columna no encontrada: '{column}' (no existe en el esquema de la tabla; revisa las columnas de create_table)"
    )]
    ColumnNotFound {
        /// Nombre de la columna solicitada.
        column: String,
    },

    /// Los tipos de un valor y su columna (o de una comparación) no coinciden.
    #[error("tipos incompatibles: {message}")]
    TypeMismatch {
        /// Descripción accionable del desajuste (qué valor, dónde, qué se esperaba).
        message: String,
    },
}
