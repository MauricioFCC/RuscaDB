//! Tests de aceptación de la feature `otel-real` (SPEC-0155).

#![allow(clippy::expect_used)]

use ruscadb::OtelRealExporter;

#[test] // @spec AC-0155-01
fn test_ac_0155_01_otel_real_span() {
    let exporter = OtelRealExporter::new();
    let json = exporter.export_span("abc", "def", "query");
    assert!(json.contains("\"trace_id\":\"abc\""));
}
