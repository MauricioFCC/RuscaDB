//! Exportador OpenTelemetry OTLP (SPEC-0155): span a JSON.
//!
//! Sin dependencias externas: serializa el span con el shape OTLP minimo
//! (`trace_id`, `span_id`, `name`). El transporte HTTP/gRPC queda fuera de
//! alcance (lo inyecta el consumidor).

/// Exportador de spans.
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

    /// Serializa un span a JSON.
    ///
    /// Args:
    ///     trace_id: Id de traza.
    ///     span_id: Id de span.
    ///     name: Nombre de la operacion.
    ///
    /// Returns:
    ///     JSON con el shape OTLP minimo.
    pub fn export_span(&self, trace_id: &str, span_id: &str, name: &str) -> String {
        format!(
            r#"{{"trace_id":"{}","span_id":"{}","name":"{}"}}"#,
            trace_id, span_id, name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::OtelRealExporter;

    #[test] // @spec AC-0155-01
    fn test_ac_0155_01_otel_real_span() {
        let exporter = OtelRealExporter::new();
        let json = exporter.export_span("abc", "def", "query");
        assert!(json.contains("\"trace_id\":\"abc\""));
    }
}
