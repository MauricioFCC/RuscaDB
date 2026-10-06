---
id: SPEC-0006
feature: vector_index
status: accepted
owner: index-team
appetite_days: 10
boundaries:
  crates: [ruscadb-vector, ruscadb-core]
  out_of_scope: [storage, wal, query, bindings]
fr:
  - { id: FR-0006-01, desc: "índice HNSW con parámetros M, ef_construction, ef_search" }
  - { id: FR-0006-02, desc: "métricas L2, coseno y producto interno (Metric de core)" }
  - { id: FR-0006-03, desc: "insert devuelve un id de nodo; search devuelve k vecinos por distancia" }
  - { id: FR-0006-04, desc: "rechazar dimensión inconsistente con DimensionMismatch" }
nf:
  - { id: NF-0006-01, desc: "recall@10 >= 0.90 frente a fuerza bruta (N=300, dim=16)" }
  - { id: NF-0006-02, desc: "search sublineal (no fuerza bruta) en el grafo" }
acceptance_criteria:
  - id: AC-0006-01
    given: un conjunto pequeño de vectores L2
    when: se busca el vecino más cercano
    then: devuelve el id correcto
    test: test_ac_0006_01_exact_nearest_small_l2
  - id: AC-0006-02
    given: 300 vectores aleatorios y 20 consultas
    when: se compara HNSW con fuerza bruta
    then: el recall@10 promedio es >= 0.90
    test: test_ac_0006_02_recall_at_10
  - id: AC-0006-03
    given: vectores de dimensión distinta a la del índice
    when: se inserta o busca
    then: devuelve DimensionMismatch
    test: test_ac_0006_03_dimension_mismatch
  - id: AC-0006-04
    given: vectores donde coseno y L2 discrepan
    when: se busca con métrica coseno
    then: devuelve el de mayor similitud coseno
    test: test_ac_0006_04_cosine_metric
  - id: AC-0006-05
    given: vectores con distinto producto interno
    when: se busca con métrica de producto interno
    then: devuelve el de mayor producto interno
    test: test_ac_0006_05_inner_product_metric
exit_criteria:
  - cargo test -p ruscadb-vector -- test_ac_0006
  - cargo mutants -p ruscadb-vector mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir ruscadb-vector al stub
sandbox:
  - cargo test -p ruscadb-vector
---

# SPEC-0006 — Índice vectorial HNSW

## Contexto

Índice ANN de RuscaDB (`docs/RuscaDB-roadmap.md` §5.4, ADR-004). Implementación
de **HNSW** (Malkov & Yashunin 2016): grafo jerárquico, inserción con
`ef_construction`, búsqueda con `ef_search`, niveles geométricos
(`mL = 1/ln M`). Métricas reutilizadas de `ruscadb_core::Metric`.

## Criterios de aceptación

- **AC-0006-01** — exactitud en caso pequeño.
- **AC-0006-02** — recall@10 ≥ 0.90 (propiedad estadística, oráculo fuerza bruta).
- **AC-0006-03** — validación de dimensión.
- **AC-0006-04/05** — métricas coseno y producto interno.

## Trazabilidad

Tests `test_ac_0006_<nn>_*` + property test; verificado por `cargo xtask trace`.
