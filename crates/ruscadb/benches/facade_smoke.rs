//! Fachada: point-read indexado e ingest de blob 1 MiB (SPEC-0059).
//!
//! Smoke: `cargo bench -p ruscadb --bench facade_smoke -- --test`.

// Los benches no son API pública: `missing_docs` del workspace no aplica aquí
// y los `expect` de setup son el estilo canónico de criterion.
#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Filas sembradas para el point-read.
const ROWS: i64 = 10_000;
/// Clave consultada (presente en el rango sembrado).
const TARGET: i64 = 4242;
/// Tamaño del blob por ingest (1 MiB).
const BLOB_BYTES: usize = 1 << 20;

/// Abre una base temporal con `t(a INT)` indexada y `ROWS` filas.
fn open_seeded() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let mut database =
        Database::open(DbConfig::new(dir.path().join("bench.db"), 256)).expect("apertura");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
    for index in 0..ROWS {
        database
            .insert(
                "t",
                ScalarMap::from([("a".to_string(), ScalarValue::Int(index))]),
            )
            .expect("insert");
    }
    database.create_index("t", "a").expect("create_index");
    (dir, database)
}

/// Point-read: `SELECT ... WHERE a = 4242` vía índice secundario.
fn bench_point_read(c: &mut Criterion) {
    let (_dir, mut database) = open_seeded();
    c.bench_function("facade/point_read_indexed", |b| {
        b.iter(|| {
            let rows = database
                .execute(&format!("SELECT a FROM t WHERE a = {TARGET}"))
                .expect("select");
            black_box(rows)
        })
    });
}

/// Ingest de blob CAS de 1 MiB por iteración (meta §6.4: ≥ 500 MB/s).
fn bench_blob_ingest(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let mut config = DbConfig::new(dir.path().join("blob.db"), 256);
    config.blob_path = Some(dir.path().join("blobs"));
    let mut database = Database::open(config).expect("apertura");
    let blob = vec![7u8; BLOB_BYTES];
    c.bench_function("facade/blob_ingest_1mib", |b| {
        b.iter(|| {
            let hash = database.put_blob(black_box(&blob)).expect("put_blob");
            black_box(hash)
        })
    });
}

criterion_group!(benches, bench_point_read, bench_blob_ingest);
criterion_main!(benches);
