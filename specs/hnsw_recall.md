---
id: SPEC-0034
feature: hnsw_recall
status: accepted
owner: search-team
appetite_days: 3
boundaries:
  crates: [ruscadb-vector]
  out_of_scope: [optimizacion de HNSW, benchmarks criterion, pHNSW]
fr:
  - { id: FR-0034-01, desc: "oraculo de fuerza bruta top-k sobre el mismo conjunto" }
  - { id: FR-0034-02, desc: "recall@k = |indice ∩ exacto| / k promediado sobre consultas" }
  - { id: FR-0034-03, desc: "test de recall@10 >= 0.95 con un corpus determinista" }
nf:
  - { id: NF-0034-01, desc: "test determinista (semilla fija) y < 20 s" }
  - { id: NF-0034-02, desc: "sin panics; dims validadas" }
acceptance_criteria:
  - id: AC-0034-01
    given: un corpus de N vectores y M consultas (semilla fija)
    when: se calcula recall@10 (HNSW vs fuerza bruta)
    then: recall@10 >= 0.95
    test: test_ac_0034_01_recall_at_10_meets_target
  - id: AC-0034-02
    given: el mismo corpus
    when: se repite la busqueda
    then: el resultado es determinista
    test: test_ac_0034_02_search_is_deterministic
  - id: AC-0034-03
    given: un indice vacio o k=0
    when: se busca
    then: devuelve vacio sin panics
    test: test_ac_0034_03_empty_and_zero_k_are_safe
exit_criteria:
  - cargo test -p ruscadb-vector -- test_ac_0034
  - cargo mutants -p ruscadb-vector mutation score >= 70%
rollback:
  - revertir russcadb-vector
sandbox:
  - cargo test -p ruscadb-vector
---

# SPEC-0034 — Recall@10 de HNSW (exit criteria F3)

## Contexto

El roadmap F3 fija `recall@10 ≥ 0.95`. Se añade un test de calidad en
`ruscadb-vector` con un oráculo de fuerza bruta y semilla fija (determinista):
construye un HNSW con N vectores aleatorios, consulta M veces, calcula el recall
promedio frente al top-k exacto y aserta `≥ 0.95`.

## Criterios de aceptación

- **AC-0034-01** — recall objetivo.
- **AC-0034-02/03** — determinismo y fronteras.

## Trazabilidad

Tests `test_ac_0034_<nn>_*`; verificado por `cargo xtask trace`.
