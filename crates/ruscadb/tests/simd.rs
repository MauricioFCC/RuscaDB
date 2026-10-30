//! Tests de aceptacion de la feature `simd` (SPEC-0150).

#![cfg(feature = "simd")]
#![allow(clippy::expect_used)]

use ruscadb::SimdOps;

#[test] // @spec AC-0150-01
fn test_ac_0150_01_dot_product() {
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![4.0, 5.0, 6.0];
    assert!((SimdOps::dot_product(&a, &b) - 32.0).abs() < 1e-9);
    assert!((SimdOps::dot_product_compensated(&a, &b) - 32.0).abs() < 1e-9);

    let normalized = SimdOps::normalize(&[3.0, 4.0]);
    let norm: f64 = normalized.iter().map(|value| value * value).sum();
    assert!((norm - 1.0).abs() < 1e-12);
    assert_eq!(SimdOps::cosine_similarity(&[0.0], &[1.0]), 0.0);
}
