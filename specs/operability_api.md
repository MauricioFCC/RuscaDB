---
id: SPEC-0187
feature: operability_api
status: implemented
maturity: real
owner: core-team
appetite_days: 3
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/crdt_sync_real.rs
    - crates/ruscadb/src/database.rs
    - crates/ruscadb/src/features.rs
    - crates/ruscadb/src/contract.rs
  out_of_scope: [doctor CLI, resource limits, conformance suite]
fr:
  - { id: FR-0187-01, desc: "Serializacion binaria del CRDT y last_tx_id" }
  - { id: FR-0187-02, desc: "Manifiesto de features y fingerprint del contrato" }
nf:
  - { id: NF-0187-01, desc: "sin dependencias nuevas" }
acceptance_criteria:
  - id: AC-0187-01
    given: una replica CRDT con reloj y valor
    when: se serializa y deserializa con to_bytes/from_bytes
    then: conserva actor, reloj, valor y merge posterior
    test: test_ac_0187_01_crdt_bytes
  - id: AC-0187-02
    given: una base con commits
    when: se consulta last_tx_id
    then: crece y snapshot_at reconstruye la vista historica
    test: test_ac_0187_02_last_tx_id
  - id: AC-0187-03
    given: el Cargo.toml de ruscadb
    when: se compara con FEATURES
    then: todas las features declaradas estan en el manifiesto
    test: test_ac_0187_03_features_manifest
  - id: AC-0187-04
    given: la superficie publica
    when: se calcula contract::fingerprint
    then: es deterministico, no cero y hex de 16
    test: test_ac_0187_04_contract_fingerprint
exit_criteria:
  - cargo test -p ruscadb --all-features --test spec_0187_operability
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir crdt_sync_real/database/features/contract
sandbox:
  - cargo test -p ruscadb --all-features --test spec_0187_operability
---

# SPEC-0187 — APIs de operabilidad y persistencia (P1.5/P1.6/P2.2/P2.3)

## Contexto

Consumidores reportaron que el CRDT no se podia persistir, que no habia
`last_tx_id` para time-travel, que el manifiesto de features no era legible por
maquina y que el contrato no se podia fijar por hash.

## Implementacion

- `CrdtSyncReal::{to_bytes, from_bytes}`: formato `RCRD1` (actor, reloj, valor,
  stamp), validacion de cotas/UTF-8 y error ante bytes sobrantes.
- `Database::last_tx_id()`: ultimo `tx_id` asignado; combinable con
  `snapshot_at(tx_id)`.
- `ruscadb::FEATURES` / `features::features()`: manifiesto const; test contra
  `Cargo.toml`.
- `contract::fingerprint()`/`fingerprint_hex()`: FNV-1a 64 de `lib.rs`.

## Complejidad

`to_bytes/from_bytes`: `O(actores + |texto|)`.
`last_tx_id`: `O(1)`.
`fingerprint`: `O(|lib.rs|)` una vez (cacheable por el consumidor).

## Fuera de alcance

`doctor --json`, limites de recursos y conformance multi-binding quedan en el
backlog (`docs/RuscaDB-backlog.md`).
