//! Tests de aceptación de la feature `ts-aggregates` (SPEC-0145).

#![allow(clippy::expect_used)]

use ruscadb::ContinuousAggregate;

#[test] // @spec AC-0145-01
fn test_ac_0145_01_continuous_aggregate() {
    let agg = ContinuousAggregate::new("avg_1m", 60000);
    let points = vec![(0, 1.0), (1000, 2.0), (60000, 3.0), (61000, 4.0)];
    let result = agg.compute(&points);
    assert_eq!(result.len(), 2);
}
