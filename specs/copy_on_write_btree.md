---
id: SPEC-0130
feature: copy_on_write_btree
status: implemented
maturity: stub
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-storage, ruscadb-txn]
  files: [crates/ruscadb/src/cow_btree.rs, crates/ruscadb-txn/src/mvcc.rs]
  out_of_scope: [distributed, clustering, replication]
fr:
  - { id: FR-0130-01, desc: "copy-on-write B+tree para almacenamiento de heap e indices" }
  - { id: FR-0130-02, desc: "snapshot readers sin bloqueo: el escritor crea nuevas paginas, los lectores ven la version anterior" }
  - { id: FR-0130-03, desc: "reclamacion de paginas obsoletas via epoch-based reclamation (RCU)" }
nf:
  - { id: NF-0130-01, desc: "zero-copy reads: los lectores no copian datos, solo referencias" }
  - { id: NF-0130-02, desc: "crash-safety: las paginas viejas sobreviven hasta checkpoint" }
acceptance_criteria:
  - id: AC-0130-01
    given: un escritor y un lector concurrentes sobre la misma base
    when: el escritor confirma una transaccion
    then: el lector sigue viendo el estado anterior (snapshot isolation)
    test: test_ac_0130_01_cow_snapshot_isolation
  - id: AC-0130-02
    given: un escritor que confirma N transacciones
    when: se hace checkpoint
    then: las paginas obsoletas se reclaman (free-list)
    test: test_ac_0130_02_cow_reclaim_after_checkpoint
exit_criteria:
  - cargo test -p ruscadb --all-features --test cow_btree
  - cargo test --workspace --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo mutants -p ruscadb --in-diff MS_diff >= 70%
rollback:
  - revertir cow_btree.rs, database.rs, storage.rs y tests al commit previo;
    el heap paginado actual sigue funcionando (regresion cero)
sandbox:
  - cargo test -p ruscadb --all-features --test cow_btree
---

# SPEC-0130 â€” Copy-on-write B+tree (MVCC snapshot readers)

## Contexto

Frontier 2026 (redb, LMDB, redb design doc): copy-on-write B+trees permiten
MVCC con lectores concurrentes sin bloqueo. El escritor crea nuevas pÃ¡ginas;
los lectores ven la versiÃ³n anterior hasta que se publique un nuevo snapshot.

RuscaDB actual: heap paginado con lock exclusivo (SPEC-0076) y `open_readonly`
(SPEC-0109) que lee el estado persistido. No hay snapshot isolation real para
lectores cross-proceso.

## Superficie

- `crates/ruscadb/src/cow_btree.rs` â€” `CowBTree` con `insert`, `get`, `snapshot`.
- `crates/ruscadb-txn/src/mvcc.rs` â€” epoch-based reclamation (RCU).
- `crates/ruscadb/src/database.rs` â€” integraciÃ³n con `open_readonly`.

## Estado del arte

- **redb** (cberner/redb): copy-on-write B+trees, MVCC readers & writer sin
  bloqueo, zero-copy, crash-safe. InspiraciÃ³n principal.
- **LMDB**: copy-on-write B+trees, single writer, multi reader, zero-copy.
- **RocksDB**: LSM-tree con versioned SSTables, snapshot readers.
- **RuscaDB actual**: heap paginado + WAL + manifest. El heap se modifica
  in-place (dirty pages en buffer pool). No hay copy-on-write.

## DiseÃ±o propuesto

1. **Copy-on-write heap**: cada escritura crea nuevas pÃ¡ginas en lugar de
   modificar las existentes. El catÃ¡logo apunta a la nueva raÃ­z.
2. **Snapshot readers**: un lector captura el `epoch` actual y lee solo las
   pÃ¡ginas de esa epoch. El escritor incrementa el epoch en cada commit.
3. **Epoch-based reclamation**: las pÃ¡ginas viejas se liberan cuando ningÃºn
   lector las referencia (epoch < low_watermark).
4. **Crash-safety**: las pÃ¡ginas viejas sobreviven hasta checkpoint; un crash
   durante escritura no corrompe el estado anterior.

## Criterios de aceptaciÃ³n

- **AC-0130-01** â€” snapshot isolation: el lector ve el estado anterior.
- **AC-0130-02** â€” reclamaciÃ³n tras checkpoint: las pÃ¡ginas obsoletas se liberan.

## Trazabilidad

Tests `test_ac_0130_NN_*` con marcador `@spec AC-0130-NN`.

## Notas

- Refactor mayor de storage: el heap paginado actual se mantiene como fallback.
- La migraciÃ³n a CoW B+trees es incremental: primero para el heap, luego para
  los Ã­ndices.
- ADR requerido: `docs/adr/ADR-0130-cow-btree.md`.
---
