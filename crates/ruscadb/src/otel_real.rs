//! OpenTelemetry OTLP (SPEC-0155): construccion del payload OTLP/JSON.
//!
//! El **transporte** (HTTP/protobuf/gRPC) es un adapter externo (requiere
//! `opentelemetry-otlp` o un cliente HTTP); aqui se implementa el contrato
//! serializable que espera el colector (`resourceSpans`/`scopeSpans/spans`),
//! reutilizando [`crate::otel::OtelExporter`].

use crate::otel::{OtelExporter, Span};

/// Exportador OTLP (payload).
#[derive(Debug, Clone, Copy, Default)]
pub struct OtelRealExporter;

impl OtelRealExporter {
    /// Crea el exportador.
    ///
    /// Returns:
    ///     Exportador sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// Serializa un span minimo a JSON (compatibilidad).
    ///
    /// Args:
    ///     trace_id: Id de traza.
    ///     span_id: Id de span.
    ///     name: Nombre de la operacion.
    ///
    /// Returns:
    ///     JSON con `trace_id`/`span_id`/`name`.
    pub fn export_span(&self, trace_id: &str, span_id: &str, name: &str) -> String {
        format!(
            r#"{{"trace_id":"{}","span_id":"{}","name":"{}"}}"#,
            trace_id, span_id, name
        )
    }

    /// Construye el payload OTLP/JSON completo.
    ///
    /// Args:
    ///     service: Nombre del servicio (`service.name`).
    ///     spans: Spans a exportar.
    ///
    /// Returns:
    ///     JSON OTLP (`resourceSpans[].scopeSpans[].spans[]`).
    pub fn export_otlp(&self, service: &str, spans: &[Span]) -> String {
        OtelExporter::new().export_otlp(service, spans)
    }
}

#[cfg(test)]
mod tests {
    use super::OtelRealExporter;
    use crate::otel::Span;
    use std::collections::BTreeMap;

    #[test] // @spec AC-0155-01
    fn test_ac_0155_01_otel_real_span() {
        let exporter = OtelRealExporter::new();
        let json = exporter.export_span("abc", "def", "query");
        assert!(json.contains("\"trace_id\":\"abc\""));
    }

    /// El payload OTLP tiene la estructura que espera el colector.
    #[test]
    fn otlp_payload_shape() {
        let mut attributes = BTreeMap::new();
        attributes.insert("db.system".to_string(), "ruscadb".to_string());
        let span = Span {
            trace_id: "0123456789abcdef".to_string(),
            span_id: "fedcba9876543210".to_string(),
            name: "select".to_string(),
            start_ms: 1_000,
            duration_ms: 5,
            attributes,
        };
        let json = OtelRealExporter::new().export_otlp("ruscadb-edge", &[span]);
        assert!(json.contains("\"resourceSpans\""));
        assert!(json.contains("\"service.name\""));
        assert!(json.contains("\"ruscadb-edge\""));
        assert!(json.contains("\"scopeSpans\""));
        assert!(json.contains("\"startTimeUnixNano\":\"1000000000\""));
        assert!(json.contains("\"endTimeUnixNano\":\"1005000000\""));
        assert!(json.contains("\"db.system\""));
    }
}
