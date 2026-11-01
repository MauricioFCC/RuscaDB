---
id: SPEC-0188
feature: doctor_metrics
status: implemented
maturity: real
owner: core-team
appetite_days: 3
boundaries:
  crates: [ruscadb, ruscadb-cli]
  files:
    - crates/ruscadb/src/doctor.rs
    - crates/ruscadb/src/metrics.rs
    - crates/ruscadb-cli/src/main.rs
  out_of_scope: [HTTP scrape, Prometheus endpoint, limites de recursos]
fr:
  - { id: FR-0188-01, desc: "diagnostico doctor y metricas estructuradas" }
nf:
  - { id: NF-0188-01, desc: "JSON estable y sin dependencias nuevas" }
acceptance_criteria:
  - id: AC-0188-01
    given: una base abierta con datos
    when: se ejecuta Database::doctor
    then: reporta version, integridad, last_tx_id y features
    test: test_ac_0188_01_doctor
  - id: AC-0188-02
    given: una base con queries ejecutadas
    when: se consulta Database::metrics
    then: los contadores son monotonos y exponen WAL
    test: test_ac_0188_02_metrics
exit_criteria:
  - cargo test -p ruscadb --all-features --test spec_0188_doctor
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir doctor/metrics/CLI
sandbox:
  - cargo test -p ruscadb --all-features --test spec_0188_doctor
---

# SPEC-0188 — Doctor y metricas (P2.1/P2.5)

## Contexto

El consumidor pide un diagnostico JSON estable y metricas estructuradas sin
dependencias ni servidor.

## Implementacion

- `Database::doctor() -> DoctorReport` (integrity_check + schema_version +
  last_tx_id + FEATURES + details).
- `Database::metrics() -> Metrics` (queries, rows, cache hits/misses, WAL bytes,
  segmentos).
- CLI `ruscadb doctor --db <ruta> [--json]`.

## Complejidad

`doctor`: `O(WAL + heap + vector)` (una pasada de integridad).
`metrics`: `O(1)` + `metadata(WAL)`.

## Fuera de alcance

Endpoint HTTP Prometheus y limites de recursos (P2.4) siguen en backlog.
