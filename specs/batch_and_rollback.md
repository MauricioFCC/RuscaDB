---
id: SPEC-0027
feature: batch_and_rollback
status: accepted
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-txn, ruscadb-storage, ruscadb-core]
  out_of_scope: [lectores concurrentes, savepoints, backend alterno, change-feed]
fr:
  - { id: FR-0027-01, desc: "Database::insert_many(table, records): un solo catalog.load, un solo commit" }
  - { id: FR-0027-02, desc: "BufferPool::discard(page): descarta un marco sucio sin escribirlo a disco" }
  - { id: FR-0027-03, desc: "TxnManager::rollback(tx): aborta una tx en vuelo (no la publica)" }
  - { id: FR-0027-04, desc: "Database::rollback(): aborta la tx activa y descarta las paginas sucias" }
  - { id: FR-0027-05, desc: "rollback en modo cifrado es rechazado con error accionable (documentado)" }
nf:
  - { id: NF-0027-01, desc: "insert_many es mas rapido que N insert (un solo commit/fsync)" }
  - { id: NF-0027-02, desc: "rollback deja la base en el ultimo estado confirmado (sin cambios parciales)" }
acceptance_criteria:
  - id: AC-0027-01
    given: una tabla creada
    when: se llama insert_many con N registros
    then: los N son visibles tras un unico commit
    test: test_ac_0027_01_insert_many_single_commit
  - id: AC-0027-02
    given: un lote de registros
    when: uno viola el esquema
    then: la operacion falla con error accionable (y no publica a medias tras rollback)
    test: test_ac_0027_02_insert_many_error_is_actionable
  - id: AC-0027-03
    given: una transaccion en vuelo
    when: se llama rollback
    then: la tx no queda confirmada y un snapshot posterior no ve los cambios
    test: test_ac_0027_03_rollback_aborts_tx
  - id: AC-0027-04
    given: cambios sin confirmar (paginas sucias)
    when: se hace rollback
    then: las paginas vuelven al ultimo estado confirmado (discard) y la base sigue operativa
    test: test_ac_0027_04_rollback_discards_dirty_pages
  - id: AC-0027-05
    given: una base en modo cifrado
    when: se intenta rollback
    then: devuelve error accionable (no soportado en modo cifrado)
    test: test_ac_0027_05_rollback_encrypted_is_error
exit_criteria:
  - cargo test -p ruscadb -p ruscadb-txn -p ruscadb-storage -- test_ac_0027
  - cargo mutants -p ruscadb -p ruscadb-txn mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir la fachada/storage/txn al commit previo
sandbox:
  - cargo test -p ruscadb -p ruscadb-txn -p ruscadb-storage
---

# SPEC-0027 — Refactor: escritura por lotes y rollback

## Contexto

Refactor guiado por comparables (sled `apply_batch`/`Tree::transaction`, redb
`WriteTransaction::abort`, duckdb-rs `Appender` y rollback al `Drop`):

- **`insert_many`**: hoy `insert_record` hace `Catalog::load` + `heap_insert` +
  `catalog.save` + `commit` **por fila** (un fsync por fila). El lote hace una
  sola carga de catálogo y un solo commit.
- **`rollback`**: RuscaDB no tiene camino de aborto. Se añade
  `BufferPool::discard`, `TxnManager::rollback` y `Database::rollback` (aborta la
  tx activa y descarta las páginas sucias, volviendo al último estado
  confirmado). En modo cifrado las páginas nunca se publican a `.data`, por lo
  que el rollback no aplica → error accionable documentado.

## Criterios de aceptación

- **AC-0027-01/02** — lote con un commit; error accionable.
- **AC-0027-03/04** — rollback aborta y descarta.
- **AC-0027-05** — rollback en cifrado → error.

## Trazabilidad

Tests `test_ac_0027_<nn>_*`; verificado por `cargo xtask trace`.
