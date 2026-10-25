---
id: SPEC-0131
feature: begin_concurrent
status: implemented
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb/src/concurrent_txn.rs, crates/ruscadb-txn/src/mvcc.rs]
  out_of_scope: [distributed, clustering, replicacion]
fr:
  - { id: FR-0131-01, desc: "BEGIN CONCURRENT: multi-writer con deteccion de conflictos por fila" }
  - { id: FR-0131-02, desc: "snapshot isolation: el lector ve el estado al BEGIN" }
  - { id: FR-0131-03, desc: "row-level conflict detection en commit (first-committer-wins)" }
nf:
  - { id: NF-0131-01, desc: "retryable Busy/BusySnapshot errors en todos los SDKs" }
acceptance_criteria:
  - id: AC-0131-01
    given: dos writers concurrentes sobre filas disjuntas
    when: ambos hacen commit
    then: ambos tienen exito (sin bloqueo)
    test: test_ac_0131_01_disjoint_writers_commit
  - id: AC-0131-02
    given: dos writers sobre la misma fila
    when: ambos hacen commit
    then: el segundo recibe BusySnapshot (retryable)
    test: test_ac_0131_02_same_row_conflict
exit_criteria:
  - cargo test -p ruscadb --all-features --test begin_concurrent
  - cargo test --workspace --all-features
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir concurrent_txn.rs y mvcc.rs al commit previo
sandbox:
  - cargo test -p ruscadb --all-features --test begin_concurrent
---

# SPEC-0131 — BEGIN CONCURRENT (multi-writer)

## Contexto

Frontier 2026 (SQLRite, rust_sqlite): `BEGIN CONCURRENT` permite multi-writer
con detección de conflictos por fila en commit. RuscaDB es single-writer
exclusivo (SPEC-0076). Esta spec añade multi-writer detrás de un feature flag.

## Superficie

- `crates/ruscadb/src/concurrent_txn.rs` — `begin_concurrent`, `commit_concurrent`.
- `crates/ruscadb-txn/src/mvcc.rs` — version index en memoria, row-level conflict detection.

## Estado del arte

- **SQLRite** (sqlritedb.com): BEGIN CONCURRENT con snapshot isolation,
  row-level conflict detection at commit, retryable Busy/BusySnapshot.
- **rust_sqlite** (joaoh82): multi-handle MVCC con logical clock.
- **RuscaDB actual**: single-writer exclusivo, MVCC snapshot isolation,
  `TxnManager` con first-committer-wins por clave.

## Diseño propuesto

1. **Version index en memoria**: cada fila tiene `created_by` y `deleted_by`
   (tx_id). El lector captura el snapshot al BEGIN.
2. **BEGIN CONCURRENT**: crea una transacción con snapshot aislado. No toma
   lock exclusivo.
3. **Commit**: detecta conflictos write-write (first-committer-wins). Si hay
   conflicto, devuelve `BusySnapshot` (retryable).
4. **Reconciliación**: el caller reintenta con un nuevo snapshot.

## Criterios de aceptación

- **AC-0131-01** — writers disjuntos: ambos commit con éxito.
- **AC-0131-02** — writers sobre la misma fila: el segundo recibe BusySnapshot.

## Notas

- Requiere refactor del lock cross-proceso (SPEC-0076): el lock pasa de
  exclusivo a shared para lectores, exclusivo solo para el commit.
- ADR requerido: `docs/adr/ADR-0131-begin-concurrent.md`.
---
