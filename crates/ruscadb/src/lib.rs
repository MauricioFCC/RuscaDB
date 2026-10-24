//! # ruscadb
//!
//! Fachada y **composition root** de RuscaDB: cablea los adapters a los
//! puertos del dominio (inyección de dependencias). Es el único crate que
//! conoce las implementaciones concretas.
//!
//! Diseño: `docs/RuscaDB-roadmap.md` §4.2/§4.3.

#![forbid(unsafe_code)]

mod auth;
mod batch;
mod blob;
mod blob_pipeline;
mod builder;
mod cache;
mod catalog;
mod database;
mod delete;
mod dml;
mod document;
mod encryption;
mod executor;
mod gates;
mod gc;
mod hardening;
mod heap;
mod hybrid;
mod index;
mod indexes;
mod join;
mod lock;
mod row_update;
mod table_api;
mod ts_api;

#[cfg(feature = "arrow-columnar")]
mod arrow_columnar;
#[cfg(feature = "backup")]
mod backup;
#[cfg(feature = "columnar")]
mod columnar;
#[cfg(any(feature = "compression", feature = "zstd-compression"))]
mod compression;
#[cfg(feature = "fts-advanced")]
mod fts_advanced;
#[cfg(feature = "fts-scoring")]
mod fts_scoring;
#[cfg(feature = "geo")]
mod geo;
#[cfg(feature = "incremental-backup")]
mod incremental_backup;
#[cfg(feature = "ivf-pq")]
mod ivf_pq;
#[cfg(feature = "observability")]
mod observability;
#[cfg(feature = "onnx")]
mod onnx;
#[cfg(feature = "otel")]
mod otel;
#[cfg(feature = "pgwire")]
mod pgwire;
#[cfg(feature = "pgwire-real")]
mod pgwire_real;
#[cfg(feature = "s3-backup")]
mod s3_backup;
#[cfg(feature = "spatial")]
mod spatial;
#[cfg(feature = "time-travel")]
mod time_travel;
#[cfg(feature = "ts-advanced")]
mod ts_advanced;
#[cfg(feature = "ts-retention")]
mod ts_retention;

#[cfg(feature = "arrow-columnar")]
pub use arrow_columnar::{ArrowColumnType, ArrowColumnarTable};
#[cfg(feature = "backup")]
pub use backup::Backup;
#[cfg(feature = "columnar")]
pub use columnar::{ColumnValue, ColumnarTable};
#[cfg(feature = "compression")]
pub use compression::Compressor;
#[cfg(feature = "zstd-compression")]
pub use compression::ZstdCompressor;
#[cfg(feature = "fts-advanced")]
pub use fts_advanced::{FuzzyMatcher, PhraseMatcher, Stemmer};
#[cfg(feature = "fts-scoring")]
pub use fts_scoring::FuzzyScorer;
#[cfg(feature = "geo")]
pub use geo::{Point, Rect};
#[cfg(feature = "incremental-backup")]
pub use incremental_backup::IncrementalBackup;
#[cfg(feature = "ivf-pq")]
pub use ivf_pq::IvfPqIndex;
#[cfg(feature = "observability")]
pub use observability::MetricsRegistry;
#[cfg(feature = "onnx")]
pub use onnx::OnnxModel;
#[cfg(feature = "otel")]
pub use otel::{OtelExporter, Span};
#[cfg(feature = "pgwire")]
pub use pgwire::PgWireServer;
#[cfg(feature = "pgwire-real")]
pub use pgwire_real::PgWireRealServer;
#[cfg(feature = "s3-backup")]
pub use s3_backup::S3Backup;
#[cfg(feature = "spatial")]
pub use spatial::RTree;
#[cfg(feature = "time-travel")]
pub use time_travel::TimeTravel;
#[cfg(feature = "ts-advanced")]
pub use ts_advanced::Downsampler;
#[cfg(feature = "ts-retention")]
pub use ts_retention::{RetentionPolicy, TsPoint};

pub use auth::{AuditEntry, Role};
pub use blob_pipeline::{BlobJobResult, PipelineStatus};
pub use builder::DatabaseBuilder;
pub use cache::{CacheMetrics, CostReport};

pub use catalog::{Catalog, ColumnDef, ColumnType, IndexDef, TableDef};
pub use database::{Database, DbConfig};
pub use encryption::EncryptionConfig;
pub use executor::{Plan, Row};
pub use gates::IngestionPolicy;
pub use hardening::{IntegrityReport, ShadowDivergence, ShadowReport};
pub use hybrid::{HybridHit, HybridStage, HybridWeights};
pub use ruscadb_core::{
    Edge, EdgeSet, Embedding, EmbeddingMeta, Metric, Record, RecordId, RecordMeta, RuscaError,
    ScalarMap, ScalarValue,
};
pub use ruscadb_multimodal::BlobHash;
// Keystore KMS-lite re-exportado por la fachada (SPEC-0088): los
// adapters (p. ej. `ruscadb-mcp`) dependen solo de la composición.
pub use ruscadb_crypto::keystore::Keystore;
pub use ruscadb_storage::{PAGE_SIZE, Page, PageId};
// Política de durabilidad del WAL (SPEC-0081): necesaria para configurar
// `DbConfig::fsync_policy` desde consumidores de la fachada.
pub use ruscadb_wal::FsyncPolicy;
pub use ts_api::TsRow;
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
    // @spec AC-0030-01
    fn test_ac_0030_01_public_surface_has_no_internals() {
        let source = include_str!("lib.rs");
        assert_no_internals(source);
        assert_mvp_surface(source);
    }

    /// AC-0030-02 — los tests migrados usan solo la API pública.
    ///
    /// Falla si `mvcc_soft_delete.rs` vuelve a usar `index_lookup_eq`.
    #[test]
    // @spec AC-0030-02
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
    // @spec AC-0030-03
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
