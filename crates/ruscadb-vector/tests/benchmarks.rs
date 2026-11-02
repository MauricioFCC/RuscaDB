//! Benchmark de calidad ANN: recall@10 vs ground truth exacto (SPEC-0198).

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use ruscadb_core::Metric;
use ruscadb_vector::{HnswIndex, HnswParams};

/// LCG determinista (misma semilla ⇒ misma recall).
struct Lcg(u32);

impl Lcg {
    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f64 / (1u64 << 24) as f64 - 0.5
    }

    fn vector(&mut self, dim: usize) -> Vec<f32> {
        (0..dim).map(|_| self.next_f64() as f32).collect()
    }
}

/// Distancia euclídea al cuadrado (oráculo exacto).
fn distance(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| (a - b) * (a - b)).sum()
}

/// AC-0198-01 — recall@10 >= 0.95 sobre un dataset determinista.
#[test] // @spec AC-0198-01
fn test_ac_0198_01_vector_benchmark() {
    const DIM: usize = 16;
    const ITEMS: usize = 2_000;
    const K: usize = 10;
    const QUERIES: usize = 20;
    let mut rng = Lcg(0x2545_F491);
    let mut vectors = Vec::with_capacity(ITEMS);
    let mut index = HnswIndex::new(HnswParams::new(Metric::L2), DIM).expect("hnsw");
    for _ in 0..ITEMS {
        let vector = rng.vector(DIM);
        index.insert(&vector).expect("insert");
        vectors.push(vector);
    }
    let mut hits = 0usize;
    for _ in 0..QUERIES {
        let query = rng.vector(DIM);
        let mut exact: Vec<(usize, f32)> = vectors
            .iter()
            .enumerate()
            .map(|(id, vector)| (id, distance(&query, vector)))
            .collect();
        exact.sort_by(|a, b| a.1.total_cmp(&b.1));
        let exact: Vec<usize> = exact.into_iter().take(K).map(|(id, _)| id).collect();
        let approx: Vec<usize> = index
            .search(&query, K, 64)
            .expect("search")
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        hits += approx.iter().filter(|id| exact.contains(id)).count();
    }
    let recall = hits as f64 / (QUERIES * K) as f64;
    assert!(recall >= 0.95, "recall@10={recall:.3} < 0.95");
}
