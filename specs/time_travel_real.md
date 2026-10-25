---
id: SPEC-0158
feature: time_travel_real
status: proposed
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb/src/time_travel_real.rs]
  out_of_scope: [versioning real, snapshot history, undo log]
fr:
  - { id: FR-0158-01, desc: "feature flag `time-travel-real` (off por defecto) habilita time-travel real" }
  - { id: FR-0158-02, desc: "TimeTravelReal::snapshot_at captura un snapshot consistente" }
nf:
  - { id: NF-0158-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0158-01
    given: la feature `time-travel-real` activada
    when: se crea un snapshot historico
    then: el snapshot tiene el tx_id solicitado
    test: test_ac_0158_01_time_travel_real
exit_criteria:
  - cargo test -p ruscadb --all-features --test time_travel_real
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0158 — Time-travel real (versioning)

## Contexto

Frontier 2026 (Picovolt, CrepeDB): lectura histórica por tx_id con versioning
real. RuscaDB tiene MVCC con tx_id en records (SPEC-0129 stub). Esta spec
añade time-travel real detrás de un feature flag.

## Superficie

- Feature `time-travel-real` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/time_travel_real.rs` — `TimeTravelReal` con `snapshot_at`.
- Integración futura: versioning real, snapshot history, undo log.

## Estado del arte

- **Picovolt**: time-travel, opt-in crash-safe durability.
- **CrepeDB**: fork desde cualquier snapshot, version history.
- **RuscaDB actual**: MVCC con tx_id en records, time-travel stub (SPEC-0129).

## Diseño propuesto

1. **TimeTravelReal**: captura un snapshot consistente por tx_id.
2. **Versioning**: cada record tiene created_tx/deleted_tx.
3. **Snapshot**: lee el estado "as of" un tx_id.

## Notas

- Implementación inicial: helper que captura un tx_id.
- Versioning real (snapshot history) queda como trabajo futuro.
---
