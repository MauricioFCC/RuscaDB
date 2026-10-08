---
id: SPEC-0045
feature: differential_tests
status: accepted
owner: quality-team
appetite_days: 5
boundaries:
  crates: [ruscadb]
  out_of_scope: [differential vs SQLite externo, KNN/TRAVERSE/agregados en el oraculo inicial]
fr:
  - { id: FR-0045-01, desc: "oraculo en memoria que evalua SELECT/WHERE/ORDER BY/LIMIT/proyeccion sobre filas conocidas" }
  - { id: FR-0045-02, desc: "generador determinista de datasets y queries (semilla fija)" }
  - { id: FR-0045-03, desc: "differential: resultado del executor == oraculo para N casos" }
  - { id: FR-0045-04, desc: "cobertura: filtros AND, comparaciones, NULL, ORDER BY asc/desc, LIMIT" }
nf:
  - { id: NF-0045-01, desc: "determinista y < 30 s" }
  - { id: NF-0045-02, desc: "sin panics; reporta el caso que diverge si falla" }
acceptance_criteria:
  - id: AC-0045-01
    given: un dataset y una query aleatoria
    when: se compara executor vs oraculo
    then: coinciden (mismo conjunto/orden de filas)
    test: test_ac_0045_01_differential_scan_filter
  - id: AC-0045-02
    given: queries con ORDER BY
    when: se compara
    then: el orden coincide con el oraculo
    test: test_ac_0045_02_differential_order_by
  - id: AC-0045-03
    given: queries con LIMIT y proyeccion
    when: se compara
    then: coinciden
    test: test_ac_0045_03_differential_limit_projection
  - id: AC-0045-04
    given: valores NULL en el dataset
    when: se compara
    then: la semantica de NULL coincide
    test: test_ac_0045_04_differential_nulls
  - id: AC-0045-05
    given: N=200 casos aleatorios (semilla fija)
    when: se ejecuta la suite
    then: 0 divergencias
    test: test_ac_0045_05_differential_batch
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0045
  - mutation no aplica (tests); el codigo de producto ya tiene su gate
rollback:
  - eliminar tests/differential.rs
sandbox:
  - cargo test -p ruscadb -- test_ac_0045
---

# SPEC-0045 — Differential testing del executor

## Contexto

F2 pide "differential ≥ 99.9%". Sin SQLite/DataFusion, se usa un **oráculo en
memoria** (evaluación directa en Rust de la semántica SQL) y un generador
determinista de datasets/queries; se compara el resultado del executor contra el
oráculo en 200 casos. Doble verificación (dual verification, CPD).

## Criterios de aceptación

- **AC-0045-01..04** — scan/filter, order by, limit/projection, NULL.
- **AC-0045-05** — lote de 200 casos sin divergencias.

## Trazabilidad

Tests `test_ac_0045_<nn>_*` en `crates/ruscadb/tests/differential.rs`.
