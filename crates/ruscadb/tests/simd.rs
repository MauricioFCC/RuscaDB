//! Tests de aceptación de la feature `simd` (SPEC-0150).

#![allow(clippy::expect_used)]

use ruscadb::SimdOps;

#[test] // @spec AC-0150-01
fn test_ac_0150_01_dot_product() {
    let a = vec![1.0, 2.0, 3.0];
    let b = vec![4.0, 5.0, 6.0];
    let dot = SimdOps::dot_product(&a, &b);
    assert!((dot - 32.0).abs() < 1e-9);
}
