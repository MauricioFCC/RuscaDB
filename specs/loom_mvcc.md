---
id: SPEC-0053
feature: loom_mvcc
status: implemented
owner: txn-team
appetite_days: 5
boundaries:
  crates: [ruscadb-txn]
  out_of_scope: [reemplazar Mutex/std por primitivas loom en produccion, modelado del WAL]
fr:
  - { id: FR-0053-01, desc: "dev-dependencia loom 0.7 ya declarada (workspace + ruscadb-txn)" }
  - { id: FR-0053-02, desc: "test de modelo loom: commits concurrentes sobre claves disjuntas preservan el invariante (ningun commit se pierde)" }
  - { id: FR-0053-03, desc: "test de modelo loom: snapshot concurrente con commit nunca observa escritura parcial" }
  - { id: FR-0053-04, desc: "documenta limites del modelo (que primitivas son reales vs modeladas)" }
nf:
  - { id: NF-0053-01, desc: "los tests loom corren rapido (preemption bound acotado, pocas hebras)" }
  - { id: NF-0053-02, desc: "sin regresion: suite previa de ruscadb-txn verde" }
acceptance_criteria:
  - id: AC-0053-01
    given: dos hebras que hacen commit sobre claves disjuntas
    when: loom explora las planificaciones
    then: ambos commits son visibles y el contador/LSN avanza exactamente 2
    test: test_ac_0053_01_concurrent_commits_no_loss
  - id: AC-0053-02
    given: una hebra lectora (snapshot) y una escritora (commit)
    when: loom explora las planificaciones
    then: el snapshot ve todo o nada del commit (atomicidad), nunca parcial
    test: test_ac_0053_02_snapshot_atomicity
  - id: AC-0053-03
    given: la suite de ruscadb-txn
    when: se ejecuta completa
    then: todo verde incluyendo los tests loom
    test: test_ac_0053_03_suite_green
exit_criteria:
  - cargo test -p ruscadb-txn -- test_ac_0053
  - cargo test -p ruscadb-txn (suite completa verde)
rollback:
  - revertir ruscadb-txn
sandbox:
  - cargo test -p ruscadb-txn
---

# SPEC-0053 — Loom para MVCC (concurrencia verificada)

## Contexto

Frontera en verificación concurrente (loom, tokio): modelar `TxnManager` con
`loom::model` para explorar planificaciones. Si las primitivas reales no son
modelables directamente, se abstrae lo mínimo (wrappers `cfg(loom)`) SIN cambiar
la semántica de producción, y se documenta qué es real vs modelado. `loom`
0.7 ya está en el workspace y en `Cargo.lock`; NO tocar manifests.

## Criterios de aceptación

- **AC-0053-01/02** — no-pérdida de commits y atomicidad de snapshot bajo loom.
- **AC-0053-03** — suite verde.

## Trazabilidad

Tests `test_ac_0053_<nn>_*`; verificado por `cargo xtask trace`.
