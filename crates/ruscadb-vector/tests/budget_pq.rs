//! Budget RAM HNSW + fallback cuantizado (SPEC-0058).
//!
//! Cubre AC-0058-01..03: rechazo sobre budget con estimado, recall@10 >= 0.95
//! del índice cuantizado vs fuerza bruta y reporte QPS-recall por bin.

#![allow(clippy::expect_used)]

use ruscadb_core::{Metric, RuscaError};
use ruscadb_vector::{
    HnswIndex, HnswParams, QuantizedFlatIndex, ScalarQuantizer, estimate_footprint,
};

/// Xorshift64 determinista (sin dependencias): f64 en `[0, 1)`.
struct TestRng(u64);

impl TestRng {
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (u64::MAX >> 11) as f64
    }
}

/// Vectores deterministas en `[-1, 1]`.
fn random_vectors(count: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
    let mut rng = TestRng(seed | 1);
    (0..count)
        .map(|_| (0..dim).map(|_| rng.next() as f32 * 2.0 - 1.0).collect())
        .collect()
}

/// Recall de `got` respecto de `expected`.
fn recall(expected: &[usize], got: &[usize]) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    expected.iter().filter(|id| got.contains(id)).count() as f64 / expected.len() as f64
}

/// Fuerza bruta L2: ids de los `k` más cercanos.
fn brute_force(vectors: &[Vec<f32>], query: &[f32], k: usize) -> Vec<usize> {
    let mut scored: Vec<(usize, f32)> = vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| {
            let sum: f32 = query
                .iter()
                .zip(vector)
                .map(|(a, b)| (a - b) * (a - b))
                .sum();
            (index, sum)
        })
        .collect();
    scored.sort_by(|a, b| a.1.total_cmp(&b.1));
    scored.into_iter().take(k).map(|(index, _)| index).collect()
}

/// AC-0058-01 — build/insert sobre budget => `ResourceLimit` con estimado.
///
/// Given: un budget menor que el footprint de 1 vector.
/// When: se inserta.
/// Then: `ResourceLimit` con `actual > limit` y el recurso nombrado.
#[test]
fn test_ac_0058_01_over_budget_rejected() {
    let dim = 8;
    let params = HnswParams {
        memory_budget_bytes: Some(1),
        ..HnswParams::new(Metric::L2)
    };
    let mut index = HnswIndex::new(params, dim).expect("new");
    let error = index
        .insert(&vec![0.0; dim])
        .expect_err("sobre budget debe fallar");
    match error {
        RuscaError::ResourceLimit {
            resource,
            limit,
            actual,
        } => {
            assert_eq!(resource, "hnsw_ram");
            assert_eq!(limit, 1);
            assert!(actual > limit, "el estimado debe superar el tope");
            assert_eq!(actual, estimate_footprint(1, params.m, dim));
        }
        other => panic!("se esperaba ResourceLimit, se obtuvo {other:?}"),
    }

    // Frontera: budget exactamente igual al estimado admite 1 vector.
    let exact = estimate_footprint(1, params.m, dim);
    let mut fitting = HnswIndex::new(
        HnswParams {
            memory_budget_bytes: Some(exact),
            ..HnswParams::new(Metric::L2)
        },
        dim,
    )
    .expect("new");
    fitting.insert(&vec![0.0; dim]).expect("cabe exacto");
    assert_eq!(fitting.len(), 1);
}

/// AC-0055-02-equivalente vectorial — recall@10 >= 0.95 cuantizado.
///
/// Given: set de eval 200x16 y su índice cuantizado.
/// When: se mide recall@10 vs fuerza bruta exacta.
/// Then: recall >= 0.95 y el footprint es ~dim bytes por vector.
#[test]
fn test_ac_0058_02_quantized_recall_at_10() {
    let vectors = random_vectors(200, 16, 11);
    let flat = QuantizedFlatIndex::build(vectors.clone()).expect("build");
    assert_eq!(flat.len(), 200);
    assert!(
        flat.footprint_bytes() <= 200 * 16 + 1024,
        "1 B/dim + codebook"
    );

    let queries = random_vectors(10, 16, 77);
    let mut total = 0.0;
    for query in &queries {
        let expected = brute_force(&vectors, query, 10);
        let got: Vec<usize> = flat
            .search(query, 10)
            .expect("search")
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        total += recall(&expected, &got);
    }
    let average = total / queries.len() as f64;
    assert!(average >= 0.95, "recall@10 cuantizado = {average}");
}

/// AC-0058-03 — QPS-recall por bin `{exacto, cuantizado} x k`.
///
/// Given: ambos índices sobre el mismo set.
/// When: se corre cada bin.
/// Then: reporta QPS y recall por bin; asserts de recall, sin crash.
#[test]
fn test_ac_0058_03_qps_recall_by_selectivity_bin() {
    let vectors = random_vectors(200, 16, 5);
    let mut exact = HnswIndex::new(HnswParams::new(Metric::L2), 16).expect("new");
    for vector in &vectors {
        exact.insert(vector).expect("insert");
    }
    let flat = QuantizedFlatIndex::build(vectors.clone()).expect("build");
    let queries = random_vectors(10, 16, 9);

    for key in [1usize, 5, 10] {
        let start = std::time::Instant::now();
        let mut total = 0.0;
        for query in &queries {
            let expected = brute_force(&vectors, query, key);
            let got: Vec<usize> = exact
                .search(query, key, 64)
                .expect("search")
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            total += recall(&expected, &got);
        }
        let elapsed = start.elapsed().as_secs_f64().max(1e-9);
        let recall_exact = total / queries.len() as f64;
        println!(
            "bin exacto k={key}: recall={recall_exact:.3} qps={:.1}",
            queries.len() as f64 / elapsed
        );
        assert!(recall_exact >= 0.95, "exacto k={key}: {recall_exact}");

        let start = std::time::Instant::now();
        let mut total = 0.0;
        for query in &queries {
            let expected = brute_force(&vectors, query, key);
            let got: Vec<usize> = flat
                .search(query, key)
                .expect("search")
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            total += recall(&expected, &got);
        }
        let elapsed = start.elapsed().as_secs_f64().max(1e-9);
        let recall_q = total / queries.len() as f64;
        println!(
            "bin cuantizado k={key}: recall={recall_q:.3} qps={:.1}",
            queries.len() as f64 / elapsed
        );
        assert!(recall_q >= 0.90, "cuantizado k={key}: {recall_q}");
    }
}

/// BVA — build vacío y dimensiones inconsistentes son errores accionables.
#[test]
fn test_ac_0058_bva_empty_and_mismatch() {
    assert!(matches!(
        QuantizedFlatIndex::build(Vec::new()),
        Err(RuscaError::InvalidConfig(_))
    ));
    assert!(matches!(
        QuantizedFlatIndex::build(vec![vec![0.0; 4], vec![0.0; 3]]),
        Err(RuscaError::DimensionMismatch { .. })
    ));
    let flat = QuantizedFlatIndex::build(vec![vec![1.0, 2.0]]).expect("build");
    assert!(flat.search(&[1.0], 1).is_err(), "dim distinta falla");
    assert!(flat.search(&[1.0, 2.0], 0).expect("k=0").is_empty());
}

/// BVA — el cuantizador hace roundtrip acotado por el paso de cuantización.
#[test]
fn test_ac_0058_bva_quantizer_roundtrip_bounded() {
    let vectors = random_vectors(50, 8, 21);
    let quantizer = ScalarQuantizer::fit(&vectors).expect("fit");
    assert_eq!(quantizer.dim(), 8);
    for vector in &vectors {
        let code = quantizer.encode(vector).expect("encode");
        assert_eq!(code.len(), 8, "1 byte por dim");
        let back = quantizer.decode(&code).expect("decode");
        for (original, approx) in vector.iter().zip(&back) {
            // Paso máximo (rango 2.0 / 255) más epsilon numérico.
            assert!(
                (original - approx).abs() <= 2.0 / 255.0 + 1e-6,
                "roundtrip acotado: {original} vs {approx}"
            );
        }
    }
}
