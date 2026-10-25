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
#[cfg(feature = "async-io")]
mod async_io;
#[cfg(feature = "backup")]
mod backup;
#[cfg(feature = "backup-verify")]
mod backup_verify;
#[cfg(feature = "branching")]
mod branching;
#[cfg(feature = "columnar")]
mod columnar;
#[cfg(any(feature = "compression", feature = "zstd-compression"))]
mod compression;
#[cfg(feature = "begin-concurrent")]
mod concurrent_txn;
#[cfg(feature = "cow-btree")]
mod cow_btree;
#[cfg(feature = "fts-advanced")]
mod fts_advanced;
#[cfg(feature = "fts-proximity")]
mod fts_proximity;
#[cfg(feature = "fts-scoring")]
mod fts_scoring;
#[cfg(feature = "geo")]
mod geo;
#[cfg(feature = "geo-proj")]
mod geo_proj;
#[cfg(feature = "incremental-backup")]
mod incremental_backup;
#[cfg(feature = "ivf-clustering")]
mod ivf_clustering;
#[cfg(feature = "ivf-pq")]
mod ivf_pq;
#[cfg(feature = "join-reorder")]
mod join_reorder;
#[cfg(feature = "kms")]
mod kms;
#[cfg(feature = "lance-format")]
mod lance_format;
#[cfg(feature = "materialized-views")]
mod materialized_view;
#[cfg(feature = "observability")]
mod observability;
#[cfg(feature = "onnx")]
mod onnx;
#[cfg(feature = "otel")]
mod otel;
#[cfg(feature = "otel-real")]
mod otel_real;
#[cfg(feature = "pgwire")]
mod pgwire;
#[cfg(feature = "cost-planner")]
mod planner_cost;
#[cfg(feature = "prepared")]
mod prepared;
#[cfg(feature = "query-cache")]
mod query_cache;
#[cfg(feature = "rcu")]
mod rcu;
#[cfg(feature = "rtree-split")]
mod rtree_split;
#[cfg(feature = "s3-backup")]
mod s3_backup;
#[cfg(feature = "schema-evolution")]
mod schema_evolution;
#[cfg(feature = "simd")]
mod simd;
#[cfg(feature = "spatial")]
mod spatial;
#[cfg(feature = "sql-std")]
mod sql_std;
#[cfg(feature = "stemming")]
mod stemming;
#[cfg(feature = "sync")]
mod sync;
#[cfg(feature = "time-travel")]
mod time_travel;
#[cfg(feature = "time-travel-real")]
mod time_travel_real;
#[cfg(feature = "ts-advanced")]
mod ts_advanced;
#[cfg(feature = "ts-aggregates")]
mod ts_aggregates;
#[cfg(feature = "ts-retention")]
mod ts_retention;
#[cfg(feature = "vle")]
mod vle;
#[cfg(feature = "zstd-real")]
mod zstd_real;

#[cfg(feature = "arrow-columnar")]
pub use arrow_columnar::{ArrowColumnType, ArrowColumnarTable};
#[cfg(feature = "async-io")]
pub use async_io::AsyncReader;
#[cfg(feature = "backup")]
pub use backup::Backup;
#[cfg(feature = "backup-verify")]
pub use backup_verify::BackupVerifier;
#[cfg(feature = "branching")]
pub use branching::Branch;
#[cfg(feature = "columnar")]
pub use columnar::{ColumnValue, ColumnarTable};
#[cfg(feature = "compression")]
pub use compression::Compressor;
#[cfg(feature = "zstd-compression")]
pub use compression::ZstdCompressor;
#[cfg(feature = "begin-concurrent")]
pub use concurrent_txn::{ConcurrentTxn, ConcurrentTxnManager};
#[cfg(feature = "cow-btree")]
pub use cow_btree::{CowBTree, CowBTreeNode};
#[cfg(feature = "fts-advanced")]
pub use fts_advanced::{FuzzyMatcher, PhraseMatcher, Stemmer};
#[cfg(feature = "fts-proximity")]
pub use fts_proximity::ProximityMatcher;
#[cfg(feature = "fts-scoring")]
pub use fts_scoring::FuzzyScorer;
#[cfg(feature = "spatial")]
pub use geo::RTree;
#[cfg(feature = "geo")]
pub use geo::{Point, Rect};
#[cfg(feature = "geo-proj")]
pub use geo_proj::GeoPoint;
#[cfg(feature = "incremental-backup")]
pub use incremental_backup::IncrementalBackup;
#[cfg(feature = "ivf-clustering")]
pub use ivf_clustering::IvfClusterer;
#[cfg(feature = "ivf-pq")]
pub use ivf_pq::IvfPqIndex;
#[cfg(feature = "join-reorder")]
pub use join_reorder::JoinReorderer;
#[cfg(feature = "kms")]
pub use kms::KmsProvider;
#[cfg(feature = "lance-format")]
pub use lance_format::LanceTable;
#[cfg(feature = "materialized-views")]
pub use materialized_view::MaterializedView;
#[cfg(feature = "observability")]
pub use observability::MetricsRegistry;
#[cfg(feature = "onnx")]
pub use onnx::OnnxModel;
#[cfg(feature = "otel")]
pub use otel::{OtelExporter, Span};
#[cfg(feature = "otel-real")]
pub use otel_real::OtelRealExporter;
#[cfg(feature = "pgwire")]
pub use pgwire::PgWireServer;
#[cfg(feature = "pgwire-real")]
pub use pgwire::PgWireServer as PgWireRealServer;
#[cfg(feature = "cost-planner")]
pub use planner_cost::CostEstimator;
#[cfg(feature = "prepared")]
pub use prepared::PreparedStatement;
#[cfg(feature = "query-cache")]
pub use query_cache::QueryCache;
#[cfg(feature = "rcu")]
pub use rcu::RcuDomain;
#[cfg(feature = "rtree-split")]
pub use rtree_split::RTreeSplit;
#[cfg(feature = "s3-backup")]
pub use s3_backup::S3Backup;
#[cfg(feature = "schema-evolution")]
pub use schema_evolution::SchemaEvolution;
#[cfg(feature = "simd")]
pub use simd::SimdOps;
#[cfg(feature = "sql-std")]
pub use sql_std::WindowFunc;
#[cfg(feature = "stemming")]
pub use stemming::SnowballStemmer;
#[cfg(feature = "sync")]
pub use sync::SyncProtocol;
#[cfg(feature = "time-travel")]
pub use time_travel::TimeTravel;
#[cfg(feature = "time-travel-real")]
pub use time_travel_real::TimeTravelReal;
#[cfg(feature = "ts-advanced")]
pub use ts_advanced::Downsampler;
#[cfg(feature = "ts-aggregates")]
pub use ts_aggregates::ContinuousAggregate;
#[cfg(feature = "ts-retention")]
pub use ts_retention::{RetentionPolicy, TsPoint};
#[cfg(feature = "vle")]
pub use vle::VleDatabase;
#[cfg(feature = "zstd-real")]
pub use zstd_real::ZstdReal;

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
