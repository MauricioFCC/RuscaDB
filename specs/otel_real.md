---
id: SPEC-0155
feature: otel_real
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/otel_real.rs]
  out_of_scope: [OTLP gRPC, Jaeger exporter, Prometheus remote write]
fr:
  - { id: FR-0155-01, desc: "feature flag `otel-real` (off por defecto) habilita OTLP real" }
  - { id: FR-0155-02, desc: "OtelRealExporter::export_span produce un span OTLP" }
nf:
  - { id: NF-0155-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0155-01
    given: la feature `otel-real` activada
    when: se exporta un span
    then: el resultado es un JSON OTLP valido
    test: test_ac_0155_01_otel_real_span
exit_criteria:
  - cargo test -p ruscadb --all-features --test otel_real
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0155 — OpenTelemetry OTLP real

## Contexto

Frontier 2026 (Jaeger, Prometheus): exportación OTLP de traces y métricas.
RuscaDB tiene tracing (SPEC-0080) y métricas en memoria (SPEC-0119).
Esta spec añade un exportador OTLP básico detrás de un feature flag.

## Superficie

- Feature `otel-real` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/otel_real.rs` — `OtelRealExporter` con `export_span`.
- Integración futura: OTLP gRPC/HTTP, Jaeger exporter.

## Estado del arte

- **OpenTelemetry**: estándar de observabilidad (traces, metrics, logs).
- **OTLP**: protocolo de transporte (gRPC, HTTP/protobuf, HTTP/JSON).
- **RuscaDB actual**: tracing (SPEC-0080), métricas en memoria (SPEC-0119).

## Diseño propuesto

1. **OtelRealExporter**: exporta spans en formato OTLP JSON.
2. **Batch export**: agrupa spans para eficiencia.
3. **Resource attributes**: servicio, versión, host.

## Notas

- Implementación inicial: serialización JSON (sin gRPC real).
- OTLP gRPC/HTTP queda como trabajo futuro.
---
