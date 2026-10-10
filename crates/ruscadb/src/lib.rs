//! # ruscadb
//!
//! Fachada y **composition root** de RuscaDB: cablea los adapters a los
//! puertos del dominio (inyección de dependencias). Es el único crate que
//! conoce las implementaciones concretas.
//!
//! Diseño: `docs/RuscaDB-roadmap.md` §4.2/§4.3.

#![forbid(unsafe_code)]

mod batch;
mod blob;
mod builder;
mod catalog;
mod database;
mod delete;
mod dml;
mod document;
mod encryption;
mod executor;
mod gc;
mod heap;
mod index;
mod indexes;
mod join;
mod table_api;

pub use builder::DatabaseBuilder;

pub use catalog::{Catalog, ColumnDef, ColumnType, IndexDef, TableDef};
pub use database::{Database, DbConfig};
pub use encryption::EncryptionConfig;
pub use executor::{Plan, Row};
pub use ruscadb_core::{
    Edge, EdgeSet, Embedding, EmbeddingMeta, Metric, Record, RecordId, RecordMeta, RuscaError,
    ScalarMap, ScalarValue,
};
pub use ruscadb_multimodal::BlobHash;
pub use ruscadb_storage::{PAGE_SIZE, Page, PageId};
// Vocabulario transaccional / de manifiesto para consumidores de la fachada
// (SPEC-0019): snapshots, versiones y el manifiesto versionado.
pub use ruscadb_txn::{CURRENT_SCHEMA_VERSION, Manifest, Snapshot, TxId, Version};

/// Versión de la fachada, tomada de la del paquete.
pub const RUSCADB_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::RUSCADB_VERSION;
    use super::{ColumnDef, ColumnType, Database, DbConfig, PAGE_SIZE, Page, PageId};
    use ruscadb_core::{ScalarMap, ScalarValue};

    /// La fachada expone una versión no vacía (smoke test del esqueleto F0).
    #[test]
    fn facade_version_is_not_empty() {
        assert!(!RUSCADB_VERSION.is_empty());
    }

    /// Extrae las líneas de re-export (`pub use`) de `lib.rs`.
    fn public_reexports(source: &str) -> String {
        source
            .lines()
            .filter(|line| line.trim_start().starts_with("pub use"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Comprueba que ningún símbolo interno aparece en `lib.rs`.
    fn assert_no_internals(source: &str) {
        let exports = public_reexports(source);
        let internals = "heap_insert heap_read heap_update heap_remove heap_scan blank_slotted_page RowLocator canonical_key index_insert index_remove index_lookup_eq index_build execute_select execute_with_plan plan_for CATALOG_MAGIC CATALOG_VERSION";
        for internal in internals.split(' ') {
            assert!(
                !exports.contains(internal),
                "la API pública no debe exponer {internal}"
            );
        }
    }

    /// Comprueba que la superficie MVP sigue expuesta en `lib.rs`.
    fn assert_mvp_surface(source: &str) {
        let exports = public_reexports(source);
        let expected = "Database DbConfig DatabaseBuilder Catalog TableDef ColumnDef ColumnType IndexDef Plan Row PAGE_SIZE Page PageId";
        for symbol in expected.split(' ') {
            assert!(exports.contains(symbol), "la API MVP conserva {symbol}");
        }
    }

    /// AC-0030-01 — la superficie pública no re-exporta internals.
    ///
    /// Falla si `lib.rs` vuelve a exponer `heap_*`/`index_*`/
    /// `execute_select*`/`plan_for`/`RowLocator`/`CATALOG_*`.
    #[test]
    fn test_ac_0030_01_public_surface_has_no_internals() {
        let source = include_str!("lib.rs");
        assert_no_internals(source);
        assert_mvp_surface(source);
    }

    /// AC-0030-02 — los tests migrados usan solo la API pública.
    ///
    /// Falla si `mvcc_soft_delete.rs` vuelve a usar `index_lookup_eq`.
    #[test]
    fn test_ac_0030_02_index_tests_use_public_api() {
        let source = include_str!("../tests/mvcc_soft_delete.rs");
        assert!(
            !source.contains("index_lookup_eq"),
            "los tests deben usar execute, no index_lookup_eq"
        );
        assert!(
            source.contains("SELECT * FROM t WHERE a ="),
            "la coherencia del índice se prueba vía execute con Eq"
        );
    }

    /// AC-0030-03 — la superficie MVP conservada no regresa.
    ///
    /// Ejerce `Database` + `get_record`/`primary_index_len`/`read_page`/
    /// `write_page` + `Page`/`PageId`/`PAGE_SIZE` (lo que usa `ruscadb-ffi`).
    #[test]
    fn test_ac_0030_03_no_regressions() {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join("ac0030.data");
        let mut database = Database::open(DbConfig::new(&path, 64)).expect("apertura");
        database
            .create_table(
                "t",
                vec![ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                }],
            )
            .expect("create_table");
        database.create_index("t", "a").expect("create_index");
        let mut scalars = ScalarMap::new();
        scalars.insert("a".to_string(), ScalarValue::Int(7));
        let id = database.insert("t", scalars).expect("insert");
        assert_eq!(database.primary_index_len("t"), 1);
        assert!(database.get_record("t", &id).expect("get").is_some());
        let rows = database
            .execute("SELECT * FROM t WHERE a = 7")
            .expect("select");
        assert_eq!(rows.len(), 1);
        let page = database.read_page(PageId(0)).expect("read");
        assert_eq!(page.data().len(), PAGE_SIZE);
        let page_copy = Page::new(page.id());
        database.write_page(&page_copy).expect("write");
    }
}
