---
id: SPEC-0118
feature: timeseries_advanced
status: proposed
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-ts]
  files: [crates/ruscadb/src/ts_advanced.rs]
  out_of_scope: [continuous aggregates, retention policies automáticas]
fr:
  - { id: FR-0118-01, desc: "feature flag `ts-advanced` (off por defecto) habilita downsampling y agregación" }
  - { id: FR-0118-02, desc: "Downsampler::downsample reduce resolución de series temporales" }
nf:
  - { id: NF-0118-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0118-01
    given: la feature `ts-advanced` activada
    when: se hace downsampling de una serie
    then: el resultado tiene menos puntos que el original
    test: test_ac_0118_01_downsample
exit_criteria:
  - cargo test -p ruscadb --all-features --test ts_advanced
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0118 — Time-series avanzado (downsampling, retention)

## Contexto

Frontier 2026 (TimescaleDB, InfluxDB): downsampling y retention para series
temporales. RuscaDB tiene bucketing y ventanas (SPEC-0057). Esta spec añade
downsampling básico detrás de un feature flag.

## Superficie

- Feature `ts-advanced` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/ts_advanced.rs` — `Downsampler` con `downsample`.
- Integración futura: continuous aggregates, retention policies.

## Notas

- Implementación inicial: downsampling por promedio en ventanas fijas.
- Continuous aggregates y retention quedan como trabajo futuro.
---
