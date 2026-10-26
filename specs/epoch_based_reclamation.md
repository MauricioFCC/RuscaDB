---
id: SPEC-0146
feature: epoch_based_reclamation
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb/src/rcu.rs]
  out_of_scope: [distributed, clustering]
fr:
  - { id: FR-0146-01, desc: "epoch-based reclamation (RCU) para paginas copy-on-write" }
  - { id: FR-0146-02, desc: "EpochGuard captura el epoch actual al entrar" }
  - { id: FR-0146-03, desc: "reclaim libera paginas con epoch < low_watermark" }
nf:
  - { id: NF-0146-01, desc: "zero-copy reads: los lectores no copian datos" }
acceptance_criteria:
  - id: AC-0146-01
    given: un escritor y un lector concurrentes
    when: el escritor confirma una transaccion
    then: el lector sigue viendo el estado anterior (epoch-based)
    test: test_ac_0146_01_rcu_snapshot
exit_criteria:
  - cargo test -p ruscadb --all-features --test rcu
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0146 — Epoch-based reclamation (RCU)

## Contexto

Frontier 2026 (Linux kernel RCU, left-right lock): epoch-based reclamation
permite liberar memoria/páginas sin bloquear lectores. El escritor incrementa
el epoch; los lectores capturan el epoch al entrar; las páginas se liberan
cuando todos los lectores han salido del epoch anterior.

RuscaDB actual: `reap` purga filas obsoletas pero no usa epoch-based
reclamation para páginas copy-on-write.

## Superficie

- `crates/ruscadb/src/rcu.rs` — `EpochGuard`, `RcuDomain`.
- Integración con copy-on-write B+tree (SPEC-0130).

## Estado del arte

- **Linux kernel RCU**: readers no bloquean, writers sincronizan.
- **left-right lock**: dos copias, readers siempre consistentes.
- **redb**: epoch-based reclamation para páginas CoW.
- **RuscaDB actual**: `reap` con watermark MVCC, sin epoch-based.

## Diseño propuesto

1. **EpochGuard**: captura el epoch actual al entrar, lo restaura al salir.
2. **RcuDomain**: gestiona epochs, low_watermark, y la free-list de páginas.
3. **reclaim**: libera páginas con epoch < low_watermark.
4. **Sincronización**: el escritor espera a que todos los lectores salgan del
   epoch anterior antes de liberar.

## Criterios de aceptación

- **AC-0146-01** - RCU snapshot: el lector ve el estado del epoch capturado.

## Notas

- Implementación inicial: stub sin sincronización real.
- RCU real queda como trabajo futuro (requiere atomics y memory ordering).
- ADR requerido: `docs/adr/ADR-0146-rcu.md`.
---
