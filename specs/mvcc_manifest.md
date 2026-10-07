---
id: SPEC-0018
feature: mvcc_manifest
status: accepted
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb-txn, ruscadb-core]
  out_of_scope: [integracion en la fachada (siguiente iteracion), GC/reaper, SSI]
fr:
  - { id: FR-0018-01, desc: "manifiesto versionado con schema_version, epoch y checkpoint_lsn" }
  - { id: FR-0018-02, desc: "escritura atomica del manifiesto (tmp + fsync + rename) y carga con validacion" }
  - { id: FR-0018-03, desc: "gestor MVCC: begin() asigna tx_id monotono; commit() publica; snapshot() fija visibilidad" }
  - { id: FR-0018-04, desc: "regla de visibilidad: version visible si created_tx commited y <= snapshot y no borrada a la vista" }
  - { id: FR-0018-05, desc: "deteccion de manifiesto corrupto con error accionable" }
nf:
  - { id: NF-0018-01, desc: "sin dirty reads: una version de una tx en vuelo no es visible para otra tx" }
  - { id: NF-0018-02, desc: "sin panics ante JSON arbitrario (proptest)" }
acceptance_criteria:
  - id: AC-0018-01
    given: un manifiesto con epoch y checkpoint_lsn
    when: se guarda y se vuelve a cargar
    then: los campos son identicos (roundtrip)
    test: test_ac_0018_01_manifest_roundtrip
  - id: AC-0018-02
    given: un manifiesto cargado
    when: se incrementa el epoch
    then: el epoch sube en 1 y persiste
    test: test_ac_0018_02_manifest_epoch_bump
  - id: AC-0018-03
    given: una version creada por una tx en vuelo
    when: otra tx toma un snapshot
    then: la version NO es visible (sin dirty reads)
    test: test_ac_0018_03_snapshot_hides_in_flight
  - id: AC-0018-04
    given: una version creada y confirmada por una tx
    when: una tx posterior toma un snapshot
    then: la version es visible y las borradas dejan de serlo
    test: test_ac_0018_04_commit_makes_visible
  - id: AC-0018-05
    given: un fichero de manifiesto con JSON invalido
    when: se carga
    then: devuelve error accionable (sin panics)
    test: test_ac_0018_05_corrupt_manifest_is_error
exit_criteria:
  - cargo test -p ruscadb-txn -- test_ac_0018
  - cargo mutants -p ruscadb-txn mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - eliminar el crate ruscadb-txn del workspace
sandbox:
  - cargo test -p ruscadb-txn
---

# SPEC-0018 — MVCC snapshot isolation y manifiesto versionado

## Contexto

Núcleo transaccional de RuscaDB (`docs/RuscaDB-roadmap.md` §5.3 y ADR-010). Se
implementa en un crate puro `ruscadb-txn` (depende solo de `ruscadb-core`), sin
E/S más allá del manifiesto:

- `Manifest { schema_version: u32, epoch: u64, checkpoint_lsn: u64 }` con
  `load(path)` / `store(path)` (atómico `tmp + fsync + rename`) y `bump_epoch`.
- `TxnManager` MVCC: `begin() -> TxId`, `commit(TxId)`, `snapshot() -> Snapshot`,
  y `is_visible(&Version, &Snapshot) -> bool` con `Version { created_tx,
  deleted_tx }`.
- Regla de visibilidad (snapshot isolation): visible si `created_tx` no está en
  vuelo, `created_tx <= snapshot.tx_id`, y (`deleted_tx` es `None` o no es
  visible al snapshot).

La integración en la fachada (manifiesto al abrir/commit y snapshot en el
executor) es la iteración siguiente.

## Criterios de aceptación

- **AC-0018-01/02** — roundtrip y bump del manifiesto (atómico).
- **AC-0018-03/04** — visibilidad MVCC (sin dirty reads; commit hace visible).
- **AC-0018-05** — manifiesto corrupto → error.

## Trazabilidad

Tests `test_ac_0018_<nn>_*` + proptests; verificado por `cargo xtask trace`.
