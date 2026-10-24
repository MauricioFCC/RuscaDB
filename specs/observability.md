---
id: SPEC-0119
feature: observability
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/observability.rs]
  out_of_scope: [exportador Prometheus real, OpenTelemetry SDK]
fr:
  - { id: FR-0119-01, desc: "feature flag `observability` (off por defecto) habilita métricas en memoria" }
  - { id: FR-0119-02, desc: "MetricsRegistry registra contadores y los exporta en formato texto" }
nf:
  - { id: NF-0119-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0119-01
    given: la feature `observability` activada
    when: se incrementa un contador y se exporta
    then: el formato texto contiene el nombre y el valor
    test: test_ac_0119_01_metrics_export
exit_criteria:
  - cargo test -p ruscadb --all-features --test observability
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0119 — Observabilidad (métricas Prometheus)

## Contexto

Frontier 2026: métricas Prometheus y tracing OpenTelemetry. RuscaDB tiene
tracing (SPEC-0080) pero sin exportador. Esta spec añade un registry de
métricas en memoria detrás de un feature flag.

## Superficie

- Feature `observability` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/observability.rs` — `MetricsRegistry` con `inc` + `export`.
- Integración futura: exportador Prometheus real, OpenTelemetry.

## Notas

- Implementación inicial: contadores en memoria con formato texto Prometheus.
- Exportador real queda como trabajo futuro.
---
