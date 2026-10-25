//! Tests de aceptación de la feature `cost-planner` (SPEC-0142).

#![allow(clippy::expect_used)]

use ruscadb::CostEstimator;

#[test] // @spec AC-0142-01
fn test_ac_0142_01_cost_estimate() {
    let small = CostEstimator::estimate_scan(100, 1.0);
    let large = CostEstimator::estimate_scan(10000, 1.0);
    assert!(large > small);
}
