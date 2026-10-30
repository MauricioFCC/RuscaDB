---
id: SPEC-0139
feature: cow_branching
status: implemented
maturity: stub
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb/src/branching.rs]
  out_of_scope: [distributed, clustering, merge conflicts]
fr:
  - { id: FR-0139-01, desc: "feature flag `branching` (off por defecto) habilita branching de bases" }
  - { id: FR-0139-02, desc: "Branch::fork crea una copia de la base en un punto" }
nf:
  - { id: NF-0139-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0139-01
    given: la feature `branching` activada
    when: se hace fork de una base
    then: el fork es una copia independiente
    test: test_ac_0139_01_branch_fork
exit_criteria:
  - cargo test -p ruscadb --all-features --test branching
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0139 â€” Copy-on-write branching (fork/merge)

## Contexto

Frontier 2026 (Zeta-Lite, CrepeDB): branching de bases de datos (fork/merge/rebase).
RuscaDB tiene MVCC con tx_id. Esta spec aÃ±ade branching bÃ¡sico detrÃ¡s de un
feature flag.

## Superficie

- Feature `branching` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/branching.rs` â€” `Branch` con `fork`.
- IntegraciÃ³n futura: merge, rebase, conflict resolution.

## Estado del arte

- **Zeta-Lite** (arxiv 2609.01818): whole-database fork, merge, rebase.
- **CrepeDB**: fork desde cualquier snapshot, version history.
- **RuscaDB actual**: MVCC con tx_id, snapshot isolation.

## DiseÃ±o propuesto

1. **fork**: copia la base en un punto (tx_id).
2. **merge**: fusiona cambios de un branch al principal.
3. **rebase**: reaplica commits de un branch sobre otro.

## Notas

- ImplementaciÃ³n inicial: fork por copia de ficheros.
- Merge y rebase quedan como trabajo futuro.
---
