//! Latencia de escritura del WAL: lote de 64 appends + sync (SPEC-0059).
//!
//! Modela el group-commit de F1 (SPEC-0002): 64 registros por confirmación.
//! Smoke: `cargo bench -p ruscadb-wal --bench wal_commit -- --test`.

// Los benches no son API pública: `missing_docs` del workspace no aplica aquí
// y los `expect` de setup son el estilo canónico de criterion.
#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use ruscadb_wal::{RecordKind, Wal};

/// Payload fijo de 64 bytes (realista para un frame pequeño).
const PAYLOAD: [u8; 64] = [0xAB; 64];
/// Registros por lote de group-commit (c = 64, SPEC-0055).
const BATCH: u64 = 64;

/// Mide `append`×64 + `sync` por iteración (una WAL fresca por iteración).
fn bench_wal_group_commit(c: &mut Criterion) {
    c.bench_function("wal/group_commit_c64", |b| {
        b.iter_custom(|iterations| {
            let dir = tempfile::tempdir().expect("directorio temporal");
            let mut total = Duration::ZERO;
            for iteration in 0..iterations {
                let path = dir.path().join(format!("bench-{iteration}.wal"));
                let wal = Wal::open(&path).expect("wal");
                let start = Instant::now();
                for batch in 0..BATCH {
                    wal.append(batch, RecordKind::Commit, &PAYLOAD)
                        .expect("append");
                }
                wal.sync().expect("sync");
                total += start.elapsed();
            }
            total
        })
    });
}

criterion_group!(benches, bench_wal_group_commit);
criterion_main!(benches);
