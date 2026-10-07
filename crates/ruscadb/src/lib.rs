//! # ruscadb
//!
//! Fachada y **composition root** de RuscaDB: cablea los adapters a los
//! puertos del dominio (inyección de dependencias). Es el único crate que
//! conoce las implementaciones concretas.
//!
//! Diseño: `docs/RuscaDB-roadmap.md` §4.2/§4.3.

#![forbid(unsafe_code)]

mod catalog;
mod database;
mod encryption;
mod executor;
mod heap;
mod index;
mod table_api;

pub use catalog::{
    CATALOG_MAGIC, CATALOG_VERSION, Catalog, ColumnDef, ColumnType, IndexDef, TableDef,
};
pub use database::{Database, DbConfig};
pub use encryption::EncryptionConfig;
pub use executor::{Plan, Row, execute_select, execute_with_plan, plan_for};
pub use heap::{RowLocator, heap_insert, heap_read, heap_scan};
pub use index::{canonical_key, index_build, index_insert, index_lookup_eq};
// Re-export del vocabulario de páginas para los adapters de la frontera FFI
// (`ruscadb-ffi`, SPEC-0010 §FR-0010-01): el C-ABI trabaja con `PageId`, `Page`
// y `PAGE_SIZE` de `ruscadb-storage` sin reimplementar el motor.
pub use ruscadb_core::{RecordId, RuscaError, ScalarMap, ScalarValue};
pub use ruscadb_storage::{PAGE_SIZE, Page, PageId};

/// Versión de la fachada, tomada de la del paquete.
pub const RUSCADB_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::RUSCADB_VERSION;

    /// La fachada expone una versión no vacía (smoke test del esqueleto F0).
    #[test]
    fn facade_version_is_not_empty() {
        assert!(!RUSCADB_VERSION.is_empty());
    }
}
