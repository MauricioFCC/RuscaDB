---
id: SPEC-0129
feature: time_travel
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb/src/time_travel.rs]
  out_of_scope: [versioning real, snapshot history, undo log]
fr:
  - { id: FR-0129-01, desc: "feature flag `time-travel` (off por defecto) habilita lectura historica" }
  - { id: FR-0129-02, desc: "TimeTravel::snapshot_at devuelve un snapshot con tx_id dado" }
nf:
  - { id: NF-0129-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0129-01
    given: la feature `time-travel` activada
    when: se crea un snapshot historico
    then: el snapshot tiene el tx_id solicitado
    test: test_ac_0129_01_time_travel_snapshot
exit_criteria:
  - cargo test -p ruscadb --all-features --test time_travel
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0129 — Time-travel (read as of tx_id)

## Contexto

Frontier 2026 (Picovolt, CrepeDB): lectura histórica por tx_id.
RuscaDB tiene MVCC con tx_id en records. Esta spec añade un helper de
time-travel detrás de un feature flag.

## Superficie

- Feature `time-travel` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/time_travel.rs` — `TimeTravel` con `snapshot_at`.
- Integración futura: versioning real, snapshot history.

## Notas

- Implementación inicial: helper que captura un tx_id para consultas "as of".
- Versioning real queda como trabajo futuro.
---
