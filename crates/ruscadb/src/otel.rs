#![allow(missing_docs)]
#![allow(dead_code)]
//! OpenTelemetry OTLP exporter (SPEC-0126): serialización JSON de spans.

use std::collections::BTreeMap;

/// Span de tracing.
#[derive(Debug, Clone)]
pub struct Span {
    /// ID de traza.
    pub trace_id: String,
    /// ID de span.
    pub span_id: String,
    /// Nombre de la operación.
    pub name: String,
    /// Timestamp de inicio (ms).
    pub start_ms: u64,
    /// Duración (ms).
    pub duration_ms: u64,
    /// Atributos.
    pub attributes: BTreeMap<String, String>,
}

/// Exportador OTLP (stub: serialización JSON).
#[derive(Debug, Clone, Default)]
pub struct OtelExporter;

impl OtelExporter {
    /// Crea un exportador.
    pub fn new() -> Self {
        Self
    }

    /// Exporta un span como JSON.
    pub fn export_span(&self, span: &Span) -> String {
        let mut attrs = String::new();
        for (k, v) in &span.attributes {
            if !attrs.is_empty() {
                attrs.push(',');
            }
            attrs.push_str(&format!("\"{}\":\"{}\"", k, v));
        }
        format!(
            r#"{{"trace_id":"{}","span_id":"{}","name":"{}","start_ms":{},"duration_ms":{},"attributes":{{{}}}}}"#,
            span.trace_id, span.span_id, span.name, span.start_ms, span.duration_ms, attrs
        )
    }

    /// Exporta múltiples spans.
    pub fn export_spans(&self, spans: &[Span]) -> String {
        let mut out = String::from("[");
        for (i, span) in spans.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&self.export_span(span));
        }
        out.push(']');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0126-01
    fn test_ac_0126_01_otel_span() {
        let exporter = OtelExporter::new();
        let span = Span {
            trace_id: "abc123".to_string(),
            span_id: "def456".to_string(),
            name: "query".to_string(),
            start_ms: 1000,
            duration_ms: 50,
            attributes: BTreeMap::new(),
        };
        let json = exporter.export_span(&span);
        assert!(json.contains("\"trace_id\":\"abc123\""));
        assert!(json.contains("\"span_id\":\"def456\""));
    }
}
