---
id: SPEC-0195
feature: mvcc_aborted_reclamation
status: proposed
maturity: stub
owner: storage-team
appetite_days: 3
boundaries:
  crates: [ruscadb-txn]
  files:
    - crates/ruscadb-txn/src/mvcc.rs
    - crates/ruscadb-txn/tests/mvcc_contention.rs
  out_of_scope: [undo log persistente, recuperación de abortos tras crash]
fr:
  - { id: FR-0195-01, desc: "TxnManager acota el conjunto aborted con un horizonte de snapshots vivos" }
  - { id: FR-0195-02, desc: "purga aborted < min_snapshot sin cambiar la visibilidad de rollback" }
  - { id: FR-0195-03, desc: "expone aborted_len/oldest_aborted para métricas y tests" }
nf:
  - { id: NF-0195-01, desc: "sin dependencias nuevas; O(log A) por operación" }
  - { id: NF-0195-02, desc: "memoria O(A_activas), no O(histórico)" }
acceptance_criteria:
  - id: AC-0195-01
    given: 10 000 transacciones abortadas sin snapshots vivos
    when: se crean y descartan snapshots
    then: aborted_len se mantiene acotado (no crece con el histórico)
    test: test_ac_0195_01_aborted_bounded
  - id: AC-0195-02
    given: una versión creada por una transacción abortada
    when: un snapshot posterior consulta la versión
    then: sigue invisible (no reaparece tras la purga)
    test: test_ac_0195_02_visibility_preserved
  - id: AC-0195-03
    given: un snapshot vivo anterior al aborto
    when: se intenta purgar su horizonte
    then: la entrada no se purga hasta que el snapshot se libera
    test: test_ac_0195_03_snapshot_horizon
exit_criteria:
  - cargo test -p ruscadb-txn --all-features
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - revertir el commit; el conjunto aborted vuelve a crecer sin purga
sandbox:
  - cargo test -p ruscadb-txn --test mvcc_contention
---

# SPEC-0195 — Reclaim del conjunto `aborted` de MVCC

## Contexto

SPEC-0193 añadió `TxnManager::aborted: BTreeSet<TxId>` para que un rollback
fuera observable (`Snapshot::is_visible` excluye versiones abortadas). El
conjunto no se purga: en un proceso de larga vida crece con cada rollback.
PostgreSQL resuelve el problema equivalente con `VACUUM` y un horizonte de
snapshots (versiones muertas por debajo del snapshot más antiguo).

## Diseño

1. `Snapshot` mantiene un `SnapshotGuard` (id + `Arc`/registro) que se
   deregistra al soltarse.
2. `TxnManager` mantiene `active_snapshots: BTreeMap<SnapshotId, TxId>` y
   calcula `min_snapshot = min(tx_id de snapshots vivos)`.
3. En `commit`/`rollback`/`snapshot`, purga `aborted` con
   `tx < min_snapshot` (o `tx <= next_tx` si no hay snapshots vivos).
4. `aborted_len()`/`oldest_aborted()` para tests y métricas.
5. `Snapshot::at(tx_id)` (time travel) no registra guard; documenta que
   snapshots históricos pueden requerir conservar `aborted` hasta su uso.

Invariante: `is_visible` es idéntica antes y después de la purga para todo
snapshot vivo; una versión abortada nunca reaparece.

## Complejidad

```text
aborted insert/remove: O(log A)
purga por commit:      O(k log A)  (k entradas antiguas)
memoria:               O(A_activas) en vez de O(A_histórico)
```

## TDD adversarial

- 10k abortos + snapshots descartados ⇒ `aborted_len` acotado.
- Oráculo congelado: comparar `is_visible` antes/después de la purga para
  snapshots vivos.
- Test de horizonte: snapshot antiguo impide purgar su tx.
- PBT: para cualquier secuencia de begin/commit/rollback/snapshot, una versión
  abortada jamás es visible.

## Riesgos

- Snapshots `Clone` (Arc) deben deregistrar solo al soltar el último clon.
- `Snapshot::at` histórico no tiene guard; se documenta el coste de retención.
- No cubre abortos tras crash (sin undo log persistente); fuera de alcance.
