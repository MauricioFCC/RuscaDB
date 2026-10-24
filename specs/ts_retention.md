---
id: SPEC-0122
feature: ts_retention
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-ts]
  files: [crates/ruscadb/src/ts_retention.rs]
  out_of_scope: [continuous aggregates, retention policies automaticas en background]
fr:
  - { id: FR-0122-01, desc: "feature flag `ts-retention` (off por defecto) habilita politicas de retencion" }
  - { id: FR-0122-02, desc: "RetentionPolicy::apply filtra puntos mas antiguos que el TTL" }
  - { id: FR-0122-03, desc: "RetentionPolicy::downsample aplica downsampling a puntos antiguos" }
nf:
  - { id: NF-0122-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0122-01
    given: la feature `ts-retention` activada
    when: se aplica una politica de retencion con TTL
    then: los puntos mas antiguos que el TTL se eliminan
    test: test_ac_0122_01_retention_ttl
exit_criteria:
  - cargo test -p ruscadb --all-features --test ts_retention
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0122 — Retention policies para time-series

## Contexto

Frontier 2026 (InfluxDB, TimescaleDB): retention policies para series temporales.
RuscaDB tiene downsampling (SPEC-0118). Esta spec añade retention (TTL +
downsampling automático) detrás de un feature flag.

## Superficie

- Feature `ts-retention` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/ts_retention.rs` — `RetentionPolicy` con `apply`.
- Integración futura: continuous aggregates, retention automática en background.

## Notas

- Implementación inicial: filtrado por timestamp + downsampling.
- Continuous aggregates quedan como trabajo futuro.
---
