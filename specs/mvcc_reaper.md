---
id: SPEC-0025
feature: mvcc_reaper
status: accepted
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb-txn, ruscadb-core]
  out_of_scope: [reaper en la fachada, purga fisica en disco, compactacion]
fr:
  - { id: FR-0025-01, desc: "low_watermark(): menor tx en vuelo, o next_tx si no hay ninguna" }
  - { id: FR-0025-02, desc: "is_obsolete(version, watermark): version borrada por una tx < watermark" }
  - { id: FR-0025-03, desc: "gc(versions, watermark) purga versiones obsoletas y devuelve el conteo" }
  - { id: FR-0025-04, desc: "las versiones vivas y las borradas a/por encima del watermark se conservan" }
nf:
  - { id: NF-0025-01, desc: "gc nunca purga una version visible a un snapshot >= watermark" }
  - { id: NF-0025-02, desc: "sin panics; gc de un conjunto vacio es no-op" }
acceptance_criteria:
  - id: AC-0025-01
    given: un TxnManager sin transacciones en vuelo
    when: se consulta low_watermark
    then: devuelve next_tx (todas las confirmadas son visibles)
    test: test_ac_0025_01_watermark_without_in_flight
  - id: AC-0025-02
    given: un TxnManager con transacciones en vuelo
    when: se consulta low_watermark
    then: devuelve la menor tx en vuelo
    test: test_ac_0025_02_watermark_with_in_flight
  - id: AC-0025-03
    given: versiones borradas por txs por debajo del watermark
    when: se ejecuta gc
    then: se purgan y el conteo devuelto coincide
    test: test_ac_0025_03_gc_purges_obsolete
  - id: AC-0025-04
    given: versiones vivas y versiones borradas a/por encima del watermark
    when: se ejecuta gc
    then: ninguna se purga (se conservan)
    test: test_ac_0025_04_gc_keeps_live_and_recent
  - id: AC-0025-05
    given: un conjunto vacio
    when: se ejecuta gc
    then: devuelve 0 sin panics (idempotente)
    test: test_ac_0025_05_gc_is_idempotent
exit_criteria:
  - cargo test -p ruscadb-txn -- test_ac_0025
  - cargo mutants -p ruscadb-txn mutation score >= 70%
rollback:
  - revertir russcadb-txn al commit previo
sandbox:
  - cargo test -p ruscadb-txn
---

# SPEC-0025 — Reaper MVCC (low watermark + GC de versiones)

## Contexto

El roadmap (§5.3) define el *reaper* por `low_watermark_tx` para acotar el
crecimiento de versiones MVCC. Se añade a `ruscadb-txn`:

- `TxnManager::low_watermark(&self) -> TxId`: la menor tx en vuelo (o `next_tx`
  si no hay ninguna). Cualquier versión borrada por una tx `< watermark` ya es
  invisible para todo snapshot futuro y puede purgarse.
- `pub fn is_obsolete(version: &Version, watermark: TxId) -> bool`.
- `pub fn gc(versions: &mut Vec<Version>, watermark: TxId) -> usize`.

## Criterios de aceptación

- **AC-0025-01/02** — cálculo del watermark.
- **AC-0025-03/04** — purga de obsoletas, conservación de vivas/recientes.
- **AC-0025-05** — idempotencia y no-op.

## Trazabilidad

Tests `test_ac_0025_<nn>_*`; verificado por `cargo xtask trace`.
