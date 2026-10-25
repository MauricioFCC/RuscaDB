---
id: SPEC-0145
feature: ts_continuous_aggregates
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-ts]
  files: [crates/ruscadb/src/ts_aggregates.rs]
  out_of_scope: [materialized views persistentes, refresh automatico]
fr:
  - { id: FR-0145-01, desc: "feature flag `ts-aggregates` (off por defecto) habilita agregados continuos" }
  - { id: FR-0145-02, desc: "ContinuousAggregate::compute calcula agregados por ventana" }
nf:
  - { id: NF-0145-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0145-01
    given: la feature `ts-aggregates` activada
    when: se computa un agregado continuo
    then: el resultado tiene un punto por ventana
    test: test_ac_0145_01_continuous_aggregate
exit_criteria:
  - cargo test -p ruscadb --all-features --test ts_aggregates
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0145 — Continuous aggregates para time-series

## Contexto

Frontier 2026 (TimescaleDB, InfluxDB): continuous aggregates (materialized views
pre-calculadas). RuscaDB tiene downsampling (SPEC-0118). Esta spec añade
agregados continuos detrás de un feature flag.

## Superficie

- Feature `ts-aggregates` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/ts_aggregates.rs` — `ContinuousAggregate` con `compute`.
- Integración futura: materialized views persistentes, refresh automático.

## Estado del arte

- **TimescaleDB**: continuous aggregates con refresh automático.
- **InfluxDB**: downsampling con tasks.
- **RuscaDB actual**: downsampling básico (SPEC-0118).

## Diseño propuesto

1. **ContinuousAggregate**: computa agregados (avg, min, max, sum, count) por ventana.
2. **Ventanas**: tumbling, sliding, session.
3. **Materialización**: persiste el resultado como una tabla derivada.

## Notas

- Implementación inicial: computación en memoria (sin persistencia).
- Materialized views persistentes quedan como trabajo futuro.
---
