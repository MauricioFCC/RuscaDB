//! Tests de aceptación de la feature `observability` (SPEC-0119).

#![allow(clippy::expect_used)]

use ruscadb::MetricsRegistry;

#[test] // @spec AC-0119-01
fn test_ac_0119_01_metrics_export() {
    let mut reg = MetricsRegistry::new();
    reg.inc("queries_total");
    reg.inc("queries_total");
    let exported = reg.export();
    assert!(exported.contains("queries_total 2"));
}
