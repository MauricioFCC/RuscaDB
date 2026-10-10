//! # ruscadb-vector
//!
//! Índice vectorial ANN de RuscaDB: **HNSW** (Malkov & Yashunin, 2016) con
//! métricas L2, coseno y producto interno.
//!
//! Especificación: `specs/vector_index.md` (SPEC-0006).
//! Diseño: `docs/RuscaDB-roadmap.md` §5.4 (ADR-004).

#![forbid(unsafe_code)]

mod distance;
mod hnsw;
mod quant;
mod rng;

pub use distance::distance;
pub use hnsw::{HnswIndex, HnswParams, estimate_footprint};
pub use quant::{QuantizedFlatIndex, ScalarQuantizer};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use ruscadb_core::{Metric, RuscaError};

    /// Vectores aleatorios deterministas en `[-1, 1]`.
    fn random_vectors(count: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut rng = Rng::new(seed);
        (0..count)
            .map(|_| {
                (0..dim)
                    .map(|_| (rng.next_unit() as f32) * 2.0 - 1.0)
                    .collect()
            })
            .collect()
    }

    /// Construye un índice con los vectores dados.
    fn build(vectors: &[Vec<f32>], metric: Metric) -> HnswIndex {
        let dim = vectors[0].len();
        let mut index = HnswIndex::new(HnswParams::new(metric), dim).expect("new");
        for vector in vectors {
            index.insert(vector).expect("insert");
        }
        index
    }

    /// Oráculo de fuerza bruta: ids de los `k` más cercanos.
    fn brute_force(vectors: &[Vec<f32>], query: &[f32], k: usize, metric: Metric) -> Vec<usize> {
        let mut scored: Vec<(usize, f32)> = vectors
            .iter()
            .enumerate()
            .map(|(index, vector)| (index, distance(metric, query, vector).expect("dist")))
            .collect();
        scored.sort_by(|a, b| a.1.total_cmp(&b.1));
        scored.into_iter().take(k).map(|(index, _)| index).collect()
    }

    /// Recall de `got` respecto de `expected`.
    fn recall(expected: &[usize], got: &[usize]) -> f64 {
        if expected.is_empty() {
            return 1.0;
        }
        let hits = expected.iter().filter(|id| got.contains(id)).count();
        hits as f64 / expected.len() as f64
    }

    /// AC-0006-01 — vecino más cercano exacto en un conjunto pequeño (L2).
    #[test]
    // @spec AC-0006-01
    fn test_ac_0006_01_exact_nearest_small_l2() {
        let vectors = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![0.0, 1.0],
            vec![10.0, 10.0],
        ];
        let index = build(&vectors, Metric::L2);
        let results = index.search(&[0.9, 0.0], 1, 50).expect("search");
        assert_eq!(results[0].0, 1);
        assert_eq!(index.len(), 4);
        assert!(!index.is_empty());
        assert_eq!(index.dim(), 2);
    }

    /// AC-0006-02 — recall@10 promedio >= 0.90 frente a fuerza bruta.
    #[test]
    // @spec AC-0006-02
    fn test_ac_0006_02_recall_at_10() {
        let vectors = random_vectors(300, 16, 42);
        let index = build(&vectors, Metric::L2);
        let queries = random_vectors(20, 16, 7);
        let mut total = 0.0;
        for query in &queries {
            let expected = brute_force(&vectors, query, 10, Metric::L2);
            let got: Vec<usize> = index
                .search(query, 10, 64)
                .expect("search")
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            total += recall(&expected, &got);
        }
        let average = total / queries.len() as f64;
        assert!(average >= 0.90, "recall@10 promedio = {average}");
    }

    /// AC-0006-03 — dimensión inconsistente devuelve `DimensionMismatch`.
    #[test]
    // @spec AC-0006-03
    fn test_ac_0006_03_dimension_mismatch() {
        let mut index = HnswIndex::new(HnswParams::new(Metric::L2), 4).expect("new");
        assert!(matches!(
            index.insert(&[0.0; 3]),
            Err(RuscaError::DimensionMismatch { .. })
        ));
        index.insert(&[0.0; 4]).expect("insert");
        assert!(matches!(
            index.search(&[0.0; 3], 1, 10),
            Err(RuscaError::DimensionMismatch { .. })
        ));
        assert!(distance(Metric::L2, &[0.0; 2], &[0.0; 3]).is_err());
    }

    /// AC-0006-04 — la métrica coseno elige el de mayor similitud.
    #[test]
    // @spec AC-0006-04
    fn test_ac_0006_04_cosine_metric() {
        let vectors = vec![vec![10.0, 0.0], vec![0.0, 1.0]];
        let index = build(&vectors, Metric::Cosine);
        let results = index.search(&[1.0, 0.0], 1, 10).expect("search");
        assert_eq!(results[0].0, 0);
    }

    /// AC-0006-05 — la métrica de producto interno elige el de mayor producto.
    #[test]
    // @spec AC-0006-05
    fn test_ac_0006_05_inner_product_metric() {
        let vectors = vec![vec![2.0, 0.0], vec![3.0, 0.0]];
        let index = build(&vectors, Metric::InnerProduct);
        let results = index.search(&[1.0, 0.0], 1, 10).expect("search");
        assert_eq!(results[0].0, 1);
    }

    /// Búsqueda en un índice vacío devuelve vacío.
    #[test]
    fn test_search_empty_index_returns_empty() {
        let index = HnswIndex::new(HnswParams::new(Metric::L2), 3).expect("new");
        assert!(index.is_empty());
        assert!(index.search(&[0.0; 3], 5, 10).expect("search").is_empty());
    }

    /// Distancias exactas por métrica (oráculo numérico).
    #[test]
    fn test_distance_exact_values() {
        let l2 = distance(Metric::L2, &[0.0, 0.0], &[3.0, 4.0]).expect("l2");
        assert!((l2 - 5.0).abs() < 1e-6, "L2 = {l2}");

        let cosine_same = distance(Metric::Cosine, &[1.0, 0.0], &[2.0, 0.0]).expect("cos");
        assert!(cosine_same.abs() < 1e-6, "coseno iguales = {cosine_same}");

        let cosine_orth = distance(Metric::Cosine, &[1.0, 0.0], &[0.0, 1.0]).expect("cos");
        assert!(
            (cosine_orth - 1.0).abs() < 1e-6,
            "coseno ortogonal = {cosine_orth}"
        );

        let cosine_zero = distance(Metric::Cosine, &[0.0, 0.0], &[1.0, 0.0]).expect("cos");
        assert!(
            (cosine_zero - 1.0).abs() < 1e-6,
            "coseno nulo = {cosine_zero}"
        );

        let inner = distance(Metric::InnerProduct, &[1.0, 2.0], &[3.0, 4.0]).expect("ip");
        assert!((inner + 11.0).abs() < 1e-6, "producto interno = {inner}");
    }

    /// El PRNG genera niveles variados (0 y > 0) dentro del rango.
    #[test]
    fn rng_random_level_varies() {
        let mut rng = Rng::new(12345);
        let mut saw_zero = false;
        let mut saw_nonzero = false;
        for _ in 0..2000 {
            let level = rng.random_level(16);
            assert!(level <= 32, "nivel fuera de rango: {level}");
            if level == 0 {
                saw_zero = true;
            } else {
                saw_nonzero = true;
            }
        }
        assert!(saw_zero, "debe haber niveles 0");
        assert!(saw_nonzero, "debe haber niveles > 0");
    }

    /// `next_unit` siempre cae en `[0, 1)`.
    #[test]
    fn rng_next_unit_is_in_range() {
        let mut rng = Rng::new(999);
        for _ in 0..10_000 {
            let value = rng.next_unit();
            assert!((0.0..1.0).contains(&value), "fuera de rango: {value}");
        }
    }

    /// `k == 0` devuelve vacío.
    #[test]
    fn test_search_zero_k_returns_empty() {
        let vectors = random_vectors(10, 4, 3);
        let index = build(&vectors, Metric::L2);
        assert!(index.search(&vectors[0], 0, 10).expect("search").is_empty());
    }

    proptest! {
        /// Propiedad: recall alto frente a fuerza bruta en datos aleatorios.
        #[test]
        fn prop_search_recall(seed in 0u64..2000) {
            let vectors = random_vectors(120, 8, seed | 1);
            let index = build(&vectors, Metric::L2);
            let query = random_vectors(1, 8, seed.wrapping_add(99))[0].clone();
            let expected = brute_force(&vectors, &query, 5, Metric::L2);
            let got: Vec<usize> = index
                .search(&query, 5, 64)
                .unwrap()
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            let value = recall(&expected, &got);
            prop_assert!(value >= 0.8, "recall = {}", value);
        }
    }
}
