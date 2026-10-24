//! Tests de aceptación de la feature `otel` (SPEC-0126).

#![allow(clippy::expect_used)]

use ruscadb::OtelExporter;

#[test] // @spec AC-0126-01
fn test_ac_0126_01_otel_span() {
    let exporter = OtelExporter::new();
    let span = ruscadb::Span {
        trace_id: "abc123".to_string(),
        span_id: "def456".to_string(),
        name: "query".to_string(),
        start_ms: 1000,
        duration_ms: 50,
        attributes: std::collections::BTreeMap::new(),
    };
    let json = exporter.export_span(&span);
    assert!(json.contains("\"trace_id\":\"abc123\""));
    assert!(json.contains("\"span_id\":\"def456\""));
}
