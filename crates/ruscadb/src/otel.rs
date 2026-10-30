//! OpenTelemetry OTLP exporter (SPEC-0126): serializacion JSON de spans.
//!
//! `export_span`/`export_spans` producen el shape plano historico;
//! `export_otlp` construye el payload OTLP (`resourceSpans`/`scopeSpans`).

use std::collections::BTreeMap;

/// Span de tracing.
#[derive(Debug, Clone)]
pub struct Span {
    /// ID de traza.
    pub trace_id: String,
    /// ID de span.
    pub span_id: String,
    /// Nombre de la operacion.
    pub name: String,
    /// Timestamp de inicio (ms).
    pub start_ms: u64,
    /// Duracion (ms).
    pub duration_ms: u64,
    /// Atributos.
    pub attributes: BTreeMap<String, String>,
}

/// Exportador OTLP.
#[derive(Debug, Clone, Default)]
pub struct OtelExporter;

impl OtelExporter {
    /// Crea un exportador.
    ///
    /// Returns:
    ///     Exportador sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// Exporta un span como JSON plano (compatibilidad).
    ///
    /// Args:
    ///     span: Span a serializar.
    ///
    /// Returns:
    ///     JSON con `trace_id`, `span_id`, `name`, tiempos y atributos.
    pub fn export_span(&self, span: &Span) -> String {
        let attributes: Vec<String> = span
            .attributes
            .iter()
            .map(|(key, value)| format!("\"{}\":\"{}\"", escape_json(key), escape_json(value)))
            .collect();
        format!(
            r#"{{"trace_id":"{}","span_id":"{}","name":"{}","start_ms":{},"duration_ms":{},"attributes":{{{}}}}}"#,
            escape_json(&span.trace_id),
            escape_json(&span.span_id),
            escape_json(&span.name),
            span.start_ms,
            span.duration_ms,
            attributes.join(",")
        )
    }

    /// Exporta multiples spans como array JSON plano.
    ///
    /// Args:
    ///     spans: Spans a serializar.
    ///
    /// Returns:
    ///     Array JSON.
    pub fn export_spans(&self, spans: &[Span]) -> String {
        let body: Vec<String> = spans.iter().map(|span| self.export_span(span)).collect();
        format!("[{}]", body.join(","))
    }

    /// Exporta spans con el shape OTLP/JSON (`resourceSpans`/`scopeSpans`).
    ///
    /// El transporte (HTTP/gRPC) es responsabilidad del consumidor; este metodo
    /// construye el payload que espera el colector.
    ///
    /// Args:
    ///     service: Nombre del servicio (`service.name`).
    ///     spans: Spans a exportar.
    ///
    /// Returns:
    ///     JSON OTLP valido (nanosegundos como string, atributos escapados).
    pub fn export_otlp(&self, service: &str, spans: &[Span]) -> String {
        let body: Vec<String> = spans.iter().map(otlp_span).collect();
        format!(
            r#"{{"resourceSpans":[{{"resource":{{"attributes":[{{"key":"service.name","value":{{"stringValue":"{}"}}}}]}},"scopeSpans":[{{"scope":{{"name":"ruscadb"}},"spans":[{}]}}]}}]}}"#,
            escape_json(service),
            body.join(",")
        )
    }
}

/// Serializa un span al shape OTLP (`startTimeUnixNano`/`endTimeUnixNano`).
fn otlp_span(span: &Span) -> String {
    let attributes: Vec<String> = span
        .attributes
        .iter()
        .map(|(key, value)| {
            format!(
                r#"{{"key":"{}","value":{{"stringValue":"{}"}}}}"#,
                escape_json(key),
                escape_json(value)
            )
        })
        .collect();
    format!(
        r#"{{"traceId":"{}","spanId":"{}","name":"{}","startTimeUnixNano":"{}","endTimeUnixNano":"{}","attributes":[{}]}}"#,
        escape_json(&span.trace_id),
        escape_json(&span.span_id),
        escape_json(&span.name),
        span.start_ms.saturating_mul(1_000_000),
        span.start_ms
            .saturating_add(span.duration_ms)
            .saturating_mul(1_000_000),
        attributes.join(",")
    )
}

/// Escapa una cadena para JSON.
fn escape_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control.is_control() => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{OtelExporter, Span};
    use std::collections::BTreeMap;

    #[test] // @spec AC-0126-01
    fn test_ac_0126_01_otel_span() {
        let exporter = OtelExporter::new();
        let span = Span {
            trace_id: "abc123".to_string(),
            span_id: "def456".to_string(),
            name: "query".to_string(),
            start_ms: 1_000,
            duration_ms: 50,
            attributes: BTreeMap::new(),
        };
        let json = exporter.export_span(&span);
        assert!(json.contains("\"trace_id\":\"abc123\""));
        assert!(json.contains("\"span_id\":\"def456\""));
        let array = exporter.export_spans(std::slice::from_ref(&span));
        assert!(array.starts_with('[') && array.ends_with(']'));
    }

    /// El payload OTLP tiene el shape del colector y escapa atributos.
    #[test]
    fn otlp_payload_shape_and_escaping() {
        let mut attributes = BTreeMap::new();
        attributes.insert("db.system".to_string(), "rus\"cadb".to_string());
        let span = Span {
            trace_id: "0123456789abcdef".to_string(),
            span_id: "fedcba9876543210".to_string(),
            name: "select".to_string(),
            start_ms: 1_000,
            duration_ms: 5,
            attributes,
        };
        let json = OtelExporter::new().export_otlp("edge", &[span]);
        assert!(json.contains("\"resourceSpans\""));
        assert!(json.contains("\"service.name\""));
        assert!(json.contains("\"scopeSpans\""));
        assert!(json.contains("\"startTimeUnixNano\":\"1000000000\""));
        assert!(json.contains("\"endTimeUnixNano\":\"1005000000\""));
        assert!(json.contains("rus\\\"cadb"), "atributo escapado: {json}");
    }
}
