#![allow(missing_docs)]
#![allow(dead_code)]
//! OpenTelemetry OTLP real (SPEC-0155): stub.

#[derive(Debug, Clone, Default)]
pub struct OtelRealExporter;

impl OtelRealExporter {
    pub fn new() -> Self {
        Self
    }
    pub fn export_span(&self, trace_id: &str, span_id: &str, name: &str) -> String {
        format!(
            r#"{{"trace_id":"{}","span_id":"{}","name":"{}"}}"#,
            trace_id, span_id, name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0155-01
    fn test_ac_0155_01_otel_real_span() {
        let exporter = OtelRealExporter::new();
        let json = exporter.export_span("abc", "def", "query");
        assert!(json.contains("\"trace_id\":\"abc\""));
    }
}
