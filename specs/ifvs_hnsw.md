---
id: SPEC-0047
feature: ifvs_hnsw
status: accepted
owner: search-team
appetite_days: 6
boundaries:
  crates: [ruscadb-fvs]
  out_of_scope: [integracion en el executor, PCA/pHNSW, PQ]
fr:
  - { id: FR-0047-01, desc: "search_ifvs(index, query, k, ef, allowed): busqueda in-filter sobre HNSW (aplica el filtro durante el recorrido)" }
  - { id: FR-0047-02, desc: "nunca devuelve ids fuera de allowed (sonido)" }
  - { id: FR-0047-03, desc: "recall vs fuerza bruta filtrada >= umbral para filtros moderados" }
  - { id: FR-0047-04, desc: "search_auto elige iFVS cuando 0.05 <= s < 0.6 y hay indice HNSW disponible" }
nf:
  - { id: NF-0047-01, desc: "determinista; sin panics; allowed vacio => vacio" }
  - { id: NF-0047-02, desc: "documenta el coste (ef efectivo) y la comparacion con pre-filter" }
acceptance_criteria:
  - id: AC-0047-01
    given: un HnswIndex con vectores y un filtro
    when: se hace search_ifvs
    then: todos los ids devueltos pertenecen al filtro
    test: test_ac_0047_01_ifvs_is_sound
  - id: AC-0047-02
    given: un filtro moderado
    when: se compara iFVS vs fuerza bruta filtrada
    then: el recall@k supera el umbral (>= 0.90)
    test: test_ac_0047_02_ifvs_recall
  - id: AC-0047-03
    given: un filtro vacio o k=0
    when: se busca
    then: devuelve vacio sin panics
    test: test_ac_0047_03_ifvs_boundaries
  - id: AC-0047-04
    given: selectividades distintas
    when: se usa search_auto con indice
    then: elige PreFilter/InFilter/PostFilter segun los umbrales
    test: test_ac_0047_04_ifvs_strategy_selection
  - id: AC-0047-05
    given: el mismo conjunto
    when: se repite la busqueda
    then: es determinista
    test: test_ac_0047_05_ifvs_deterministic
exit_criteria:
  - cargo test -p ruscadb-fvs -- test_ac_0047
  - cargo mutants -p ruscadb-fvs mutation score >= 70%
rollback:
  - revertir russcadb-fvs
sandbox:
  - cargo test -p ruscadb-fvs
---

# SPEC-0047 — iFVS real sobre HNSW

## Contexto

`ruscadb-fvs` implementa pre/in/post sobre un corpus plano. El roadmap pide
**iFVS** (in-filter) sobre el índice HNSW real (arXiv:2607.22922): aplicar el
filtro durante el recorrido del grafo. Se añade `search_ifvs` que usa
`HnswIndex` y `search_auto` que lo elige por selectividad cuando hay índice.

## Criterios de aceptación

- **AC-0047-01/02** — sonido y recall.
- **AC-0047-03/04/05** — fronteras, selección y determinismo.

## Trazabilidad

Tests `test_ac_0047_<nn>_*`; verificado por `cargo xtask trace`.
