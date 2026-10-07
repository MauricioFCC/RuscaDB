---
id: SPEC-0022
feature: mvcc_soft_delete
status: accepted
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  out_of_scope: [GC/reaper de versiones, purga fisica, delete en grafo/fts]
fr:
  - { id: FR-0022-01, desc: "Database::delete(table, id) marca deleted_tx (borrado logico MVCC)" }
  - { id: FR-0022-02, desc: "la fila borrada se excluye de execute y de los indices" }
  - { id: FR-0022-03, desc: "el borrado sobrevive a reopen (persistido)" }
  - { id: FR-0022-04, desc: "un snapshot anterior al borrado sigue viendo la fila (MVCC)" }
  - { id: FR-0022-05, desc: "borrar un id ausente es no-op idempotente (sin panics)" }
nf:
  - { id: NF-0022-01, desc: "delete es O(log n) via indice primario (sin scan completo si hay indice)" }
  - { id: NF-0022-02, desc: "sin dirty reads; coherencia indice<->datos tras el borrado" }
acceptance_criteria:
  - id: AC-0022-01
    given: una tabla con filas
    when: se borra una fila por id
    then: la fila desaparece de "SELECT *" y del conteo
    test: test_ac_0022_01_delete_hides_row
  - id: AC-0022-02
    given: una fila borrada y confirmada
    when: se cierra y reabre
    then: la fila sigue ausente
    test: test_ac_0022_02_delete_survives_reopen
  - id: AC-0022-03
    given: un snapshot anterior al borrado
    when: se consulta con ese snapshot
    then: la fila sigue visible (snapshot isolation)
    test: test_ac_0022_03_snapshot_before_delete_still_sees_row
  - id: AC-0022-04
    given: una tabla sin la fila indicada
    when: se intenta borrar ese id
    then: devuelve Ok(false) sin panics (idempotente)
    test: test_ac_0022_04_delete_missing_is_noop
  - id: AC-0022-05
    given: una fila borrada
    when: se ejecuta KNN/MATCH/TRAVERSE
    then: la fila borrada no aparece en ningun resultado
    test: test_ac_0022_05_deleted_row_excluded_from_indexes
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0022
  - cargo mutants -p ruscadb mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir la fachada al commit previo (sin delete)
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0022 — Borrado lógico MVCC (`Database::delete`)

## Contexto

Completa el modelo MVCC de SPEC-0019: el borrado no elimina físicamente la fila
sino que fija `record.meta.deleted_tx`, de modo que las versiones son inmutables
y la visibilidad la decide el snapshot.

- `Database::delete(table, id) -> Result<bool, RuscaError>`: localiza la fila
  (indice primario o scan), reescribe el `Record` con `deleted_tx = tx` (activa o
  auto-commit), persiste y la excluye de los índices (HNSW/CSR/invertido).
  Devuelve `Ok(true)` si existía, `Ok(false)` si no.
- El executor ya filtra por `snapshot.is_visible` (SPEC-0019), por lo que una
  fila con `deleted_tx` visible deja de aparecer.
- Reopen: `restore_txn_watermark` (SPEC-0019) mantiene la coherencia del
  watermark con `deleted_tx`.

## Criterios de aceptación

- **AC-0022-01/02** — borrado visible y persistente.
- **AC-0022-03** — snapshot anterior al borrado sigue viendo la fila.
- **AC-0022-04** — borrado idempotente de id ausente.
- **AC-0022-05** — coherencia con los índices.

## Trazabilidad

Tests `test_ac_0022_<nn>_*` en `ruscadb`; verificado por `cargo xtask trace`.
