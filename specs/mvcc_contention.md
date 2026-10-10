---
id: SPEC-0055
feature: mvcc_contention
status: implemented
owner: txn-team
appetite_days: 5
boundaries:
  crates: [ruscadb-txn]
  out_of_scope: [SSI serializable, red transaccional, deadlock detection distribuida]
fr:
  - { id: FR-0055-01, desc: "stress c=64 commits concurrentes sin perdida de commits ack'd" }
  - { id: FR-0055-02, desc: "conflicto write-write => error accionable first-committer-wins" }
  - { id: FR-0055-03, desc: "helper retry_on_conflict con backoff acotado que converge" }
  - { id: FR-0055-04, desc: "metrica dead_versions/bloat expuesta y reclamada por el reaper bajo low_watermark" }
nf:
  - { id: NF-0055-01, desc: "suite previa + loom intactos (cero regresion)" }
  - { id: NF-0055-02, desc: "stress acotado en tiempo (< 60 s en CI)" }
acceptance_criteria:
  - id: AC-0055-01
    given: 64 hebras haciendo commit concurrente sobre claves disjuntas
    when: terminan todas
    then: los 64 commits son visibles, LSN avanza exactamente 64
    test: test_ac_0055_01_no_loss_under_contention
  - id: AC-0055-02
    given: dos tx concurrentes escribiendo la misma clave
    when: ambas intentan commit
    then: una gana y la otra recibe error accionable de conflicto
    test: test_ac_0055_02_write_write_conflict_actionable
  - id: AC-0055-03
    given: una carga con conflictos esporadicos
    when: se usa retry_on_conflict
    then: converge sin perdida y con intentos acotados
    test: test_ac_0055_03_retry_converges
  - id: AC-0055-04
    given: versiones muertas tras commits solapados
    when: avanza low_watermark y corre el reaper
    then: dead_versions baja y lo vivo sigue visible
    test: test_ac_0055_04_bloat_measured_and_reaped
exit_criteria:
  - cargo test -p ruscadb-txn -- test_ac_0055
  - suite completa txn verde (incl. loom)
  - cargo mutants -p ruscadb-txn acotado MS >= 70%
rollback:
  - revertir ruscadb-txn
sandbox:
  - cargo test -p ruscadb-txn
---

# SPEC-0055 — Contención MVCC c=64 + retry + bloat (R5/R6)

## Contexto

R6 (first-committer-wins + retry) y R5 (bloat del reaper) sin evidencia de
stress. Hilos reales (no solo modelo loom de 2 hebras): 64 committers,
conflicto accionable, retry con backoff acotado y métrica de versiones
muertas. Sin `unsafe` nuevo.

## Criterios de aceptación

- **AC-0055-01/02/03** — no-pérdida, conflicto accionable, retry que converge.
- **AC-0055-04** — bloat medido y reclamado.

## Trazabilidad

Tests `test_ac_0055_<nn>_*`; verificado por `cargo xtask trace`.
