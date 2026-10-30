---
id: SPEC-0126
feature: otel_exporter
status: implemented
maturity: stub
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/otel.rs]
  out_of_scope: [OTLP real, Jaeger exporter, Prometheus remote write]
fr:
  - { id: FR-0126-01, desc: "feature flag `otel` (off por defecto) habilita exportador OpenTelemetry" }
  - { id: FR-0126-02, desc: "OtelExporter::export_span produce un JSON de span" }
nf:
  - { id: NF-0126-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0126-01
    given: la feature `otel` activada
    when: se exporta un span
    then: el resultado es un JSON valido con trace_id y span_id
    test: test_ac_0126_01_otel_span
exit_criteria:
  - cargo test -p ruscadb --all-features --test otel
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0126 â€” OpenTelemetry OTLP exporter

## Contexto

Frontier 2026 (Jaeger, Prometheus): exportaciÃ³n de traces y mÃ©tricas.
RuscaDB tiene tracing (SPEC-0080) y mÃ©tricas en memoria (SPEC-0119).
Esta spec aÃ±ade un exportador OTLP bÃ¡sico detrÃ¡s de un feature flag.

## Superficie

- Feature `otel` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/otel.rs` â€” `OtelExporter` con `export_span`.
- IntegraciÃ³n futura: OTLP real, Jaeger exporter.

## Notas

- ImplementaciÃ³n inicial: serializaciÃ³n JSON de spans (sin SDK real).
- OTLP gRPC/HTTP queda como trabajo futuro.
---
