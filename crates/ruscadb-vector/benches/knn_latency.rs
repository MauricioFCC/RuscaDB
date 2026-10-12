//! Latencia de consulta KNN sobre HNSW (SPEC-0059).
//!
//! Índice de 2000 vectores dim 128 (LCG determinista), k=10, ef=64.
//! Smoke: `cargo bench -p ruscadb-vector --bench knn_latency -- --test`.

// Los benches no son API pública: `missing_docs` del workspace no aplica aquí
// y los `expect` de setup son el estilo canónico de criterion.
#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use ruscadb_core::Metric;
use ruscadb_vector::{HnswIndex, HnswParams};

/// Dimensión de los vectores.
const DIM: usize = 128;
/// Vectores del índice.
const ITEMS: usize = 2000;
/// Vecinos por consulta.
const K: usize = 10;
/// Amplitud de búsqueda.
const EF_SEARCH: usize = 64;

/// Generador LCG determinista (semilla fija = benches reproducibles).
struct Lcg(u32);

impl Lcg {
    /// Siguiente f64 en [-0.5, 0.5).
    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f64 / (1u64 << 24) as f64 - 0.5
    }

    /// Vector de `DIM` componentes.
    fn vector(&mut self, dim: usize) -> Vec<f64> {
        (0..dim).map(|_| self.next_f64()).collect()
    }
}

/// Construye el índice una vez y mide `search` por consulta.
fn bench_knn_latency(c: &mut Criterion) {
    let mut rng = Lcg(0x2545_F491);
    let mut index = HnswIndex::new(HnswParams::new(Metric::Cosine), DIM).expect("hnsw");
    for _ in 0..ITEMS {
        let vector: Vec<f64> = rng.vector(DIM);
        index
            .insert(&vector.iter().map(|value| *value as f32).collect::<Vec<_>>())
            .expect("insert");
    }
    let query: Vec<f64> = rng.vector(DIM);
    let query: Vec<f32> = query.iter().map(|value| *value as f32).collect();
    c.bench_function("knn/hnsw_2k_dim128_k10", |b| {
        b.iter(|| {
            index
                .search(black_box(&query), K, EF_SEARCH)
                .expect("search")
        })
    });
}

criterion_group!(benches, bench_knn_latency);
criterion_main!(benches);
