---
id: SPEC-0195
feature: mvcc_aborted_reclamation
status: implemented
maturity: real
owner: storage-team
appetite_days: 3
boundaries:
  crates: [ruscadb-txn]
  files:
    - crates/ruscadb-txn/src/mvcc.rs
    - crates/ruscadb-txn/tests/mvcc_contention.rs
  out_of_scope: [undo log persistente, recuperación de abortos tras crash]
fr:
  - { id: FR-0195-01, desc: "reap purga fisicamente las versiones creadas por tx abortadas" }
  - { id: FR-0195-02, desc: "tras la purga, TxnManager olvida las tx abortadas (aborted acotado)" }
  - { id: FR-0195-03, desc: "expone aborted_tx_len() para métricas y tests" }
nf:
  - { id: NF-0195-01, desc: "sin dependencias nuevas; O(log A) por operación" }
  - { id: NF-0195-02, desc: "memoria O(abortos entre reaps), no O(histórico total)" }
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
    given: un snapshot tomado antes de un aborto
    when: se ejecuta reap
    then: el snapshot sigue viendo exactamente las mismas filas (sin reaparecer abortadas)
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

1. `TxnManager::aborted() -> &BTreeSet<TxId>` y
   `forget_aborted(tx) -> bool` (API interna).
2. `Database::reap` copia el conjunto abortado, purga del heap las versiones
   con `created_tx ∈ aborted` (además de las obsoletas por watermark) y
   después llama a `forget_aborted(tx)` para cada una.
3. `Database::aborted_tx_len()` para tests/métricas.
4. La purga respeta el watermark para versiones vivas (`collect_obsolete`
   sigue usando `MvccGcReal`); las abortadas son invisibles para todo snapshot
   y no necesitan watermark.
5. Si no hay objetivos, `reap` igualmente olvida el conjunto (no hay versiones
   físicas que ocultar).

Invariante: tras `reap`, `aborted` queda vacío y las filas vivas siguen
visibles; una versión abortada nunca reaparece.

Invariante: `is_visible` es idéntica antes y después de la purga para todo
snapshot vivo; una versión abortada nunca reaparece.

## Complejidad

```text
aborted insert/remove: O(log A)
purga por commit:      O(k log A)  (k entradas antiguas)
memoria:               O(abortos entre reaps) en vez de O(A_histórico)
```

## TDD adversarial

- 10k abortos + snapshots descartados ⇒ `aborted_len` acotado.
- Oráculo congelado: comparar `is_visible` antes/después de la purga para
  snapshots vivos.
- Test de snapshot previo: mismas filas antes/después de `reap`.
- PBT: para cualquier secuencia de begin/commit/rollback/snapshot, una versión
  abortada jamás es visible.

## Riesgos

- Snapshots `Clone` (Arc) deben deregistrar solo al soltar el último clon.
- `Snapshot::at` histórico no tiene guard; se documenta el coste de retención.
- No cubre abortos tras crash (sin undo log persistente); fuera de alcance.
