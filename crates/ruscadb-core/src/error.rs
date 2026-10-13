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

    /// La base ya está abierta por este u otro proceso (SPEC-0076).
    #[error(
        "base en uso: {path} está bloqueada por otra instancia de RuscaDB; cierra ese proceso o usa otra ruta"
    )]
    DatabaseInUse {
        /// Ruta del fichero de datos que ya tiene el lock exclusivo.
        path: String,
    },

    /// El manifiesto es válido pero su versión de esquema es más nueva que la
    /// soportada por este binario (SPEC-0077). No es corrupción: el fichero
    /// está bien, hay que actualizar el binario.
    #[error(
        "esquema no soportado: el manifiesto declara schema_version {found} y este binario solo soporta {supported}; actualiza RuscaDB y no retrocedas el binario"
    )]
    SchemaVersionTooNew {
        /// Versión de esquema encontrada en el manifiesto.
        found: u32,
        /// Versión máxima soportada por este binario.
        supported: u32,
    },

    /// No existe (o falló) una migración de esquema desde `from` hasta `to`
    /// (SPEC-0077). El manifiesto no se modifica si la migración falla.
    #[error("migración de esquema no disponible: de {from} a {to}: {reason}")]
    MigrationFailed {
        /// Versión de origen del manifiesto.
        from: u32,
        /// Versión de destino requerida.
        to: u32,
        /// Motivo accionable (WHAT + WHY + WHERE).
        reason: String,
    },

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

    /// Permiso denegado por el rol de instancia (SPEC-0066, RBAC).
    #[error("permiso denegado para '{action}': el rol de la instancia no autoriza esta operación")]
    PermissionDenied {
        /// Operación denegada (WHAT + WHERE).
        action: String,
    },

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

    /// `MATCH` se aplicó sobre una columna que no es de texto indexable.
    #[error(
        "la columna '{column}' de la tabla '{table}' no es TEXT (MATCH solo indexa columnas de texto)"
    )]
    MissingTextColumn {
        /// Tabla consultada.
        table: String,
        /// Columna sobre la que se pidió `MATCH`.
        column: String,
    },

    /// `KNN` se aplicó sobre una tabla sin vectores indexados.
    #[error(
        "la tabla '{table}' no tiene vectores para la columna '{column}' (inserta registros con embedding vía insert_record)"
    )]
    MissingVector {
        /// Tabla consultada.
        table: String,
        /// Columna de embedding pedida por `KNN`.
        column: String,
    },

    /// `TRAVERSE` se aplicó sobre una tabla sin aristas de grafo.
    #[error(
        "la tabla '{table}' no tiene aristas para la columna '{column}' (inserta registros con edges.out vía insert_record)"
    )]
    MissingGraph {
        /// Tabla consultada.
        table: String,
        /// Columna de aristas pedida por `TRAVERSE`.
        column: String,
    },

    /// Conflicto write-write MVCC (first-committer-wins, SPEC-0055).
    #[error(
        "conflicto write-write en la clave '{key}' (otra transacción concurrente confirmó primero; reintenta)"
    )]
    WriteConflict {
        /// Clave escrita por dos transacciones concurrentes.
        key: String,
    },

    /// Límite duro de recursos excedido (roadmap §6.4, SPEC-0058).
    #[error("límite de recurso '{resource}' excedido: tope {limit}, estimado {actual}")]
    ResourceLimit {
        /// Recurso limitado (p. ej. "hnsw_ram", "query_complexity").
        resource: String,
        /// Tope configurado (en las unidades del recurso).
        limit: u64,
        /// Uso estimado (en las unidades del recurso).
        actual: u64,
    },

    /// Timeout cooperativo de query agotado (SPEC-0054, §6.4).
    #[error("consulta cancelada por timeout tras {millis} ms")]
    QueryTimeout {
        /// Presupuesto total concedido en milisegundos.
        millis: u64,
    },
}
