//! # ruscadb
//!
//! Fachada y **composition root** de RuscaDB: cablea los adapters a los
//! puertos del dominio (inyección de dependencias). Es el único crate que
//! conoce las implementaciones concretas.
//!
//! Diseño: `docs/RuscaDB-roadmap.md` §4.2/§4.3.

#![forbid(unsafe_code)]

mod batch;
mod catalog;
mod database;
mod delete;
mod encryption;
mod executor;
mod heap;
mod index;
mod indexes;
mod table_api;

pub use catalog::{
    CATALOG_MAGIC, CATALOG_VERSION, Catalog, ColumnDef, ColumnType, IndexDef, TableDef,
};
pub use database::{Database, DbConfig};
pub use encryption::EncryptionConfig;
pub use executor::{
    Plan, Row, execute_select, execute_select_at, execute_with_plan, execute_with_plan_at, plan_for,
};
pub use heap::{RowLocator, heap_insert, heap_read, heap_scan, heap_update};
pub use index::{canonical_key, index_build, index_insert, index_lookup_eq, index_remove};
pub use ruscadb_core::{
    Edge, EdgeSet, Embedding, EmbeddingMeta, Metric, Record, RecordId, RecordMeta, RuscaError,
    ScalarMap, ScalarValue,
};
pub use ruscadb_storage::{PAGE_SIZE, Page, PageId};
// Vocabulario transaccional / de manifiesto para consumidores de la fachada
// (SPEC-0019): snapshots, versiones y el manifiesto versionado.
pub use ruscadb_txn::{CURRENT_SCHEMA_VERSION, Manifest, Snapshot, TxId, Version};

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
