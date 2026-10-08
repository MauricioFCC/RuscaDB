---
id: SPEC-0041
feature: graph_algorithms
status: accepted
owner: graph-team
appetite_days: 6
boundaries:
  crates: [ruscadb-graph]
  out_of_scope: [integracion en RQL, SSSP/Brandes, grafos ponderados]
fr:
  - { id: FR-0041-01, desc: "bfs(graph, start) -> orden de visita (sin repetir, respeta aristas)" }
  - { id: FR-0041-02, desc: "connected_components(graph) -> particion en componentes debilmente conexas" }
  - { id: FR-0041-03, desc: "pagerank(graph, damping, iters, tol) -> vector de scores (Brin-Page 1998, iteracion de potencia)" }
  - { id: FR-0041-04, desc: "pagerank normaliza (suma ~1) y trata nodos colgantes (dangling) redistribuyendo su masa" }
nf:
  - { id: NF-0041-01, desc: "bfs/components O(V+E); pagerank O(iters*(V+E))" }
  - { id: NF-0041-02, desc: "determinista; sin panics; grafo vacio => resultados vacios" }
acceptance_criteria:
  - id: AC-0041-01
    given: un grafo dirigido conocido
    when: se ejecuta bfs desde un nodo
    then: el orden de visita es correcto y cada nodo aparece una vez
    test: test_ac_0041_01_bfs_order
  - id: AC-0041-02
    given: un grafo con dos islas
    when: se calculan las componentes conexas
    then: se obtienen exactamente dos componentes que particionan los nodos
    test: test_ac_0041_02_connected_components
  - id: AC-0041-03
    given: un grafo donde un nodo es referenciado por muchos
    when: se ejecuta pagerank
    then: el nodo central tiene el score mas alto y la suma es ~1
    test: test_ac_0041_03_pagerank_ranks_hub
  - id: AC-0041-04
    given: un grafo con un nodo sin aristas de salida (dangling)
    when: se ejecuta pagerank
    then: converge sin NaN y la suma sigue siendo ~1
    test: test_ac_0041_04_pagerank_dangling
  - id: AC-0041-05
    given: un grafo vacio o de un nodo
    when: se ejecutan los algoritmos
    then: devuelven resultados vacios/triviales sin panics
    test: test_ac_0041_05_boundary_graphs
exit_criteria:
  - cargo test -p ruscadb-graph -- test_ac_0041
  - cargo mutants -p ruscadb-graph mutation score >= 70%
rollback:
  - revertir russcadb-graph
sandbox:
  - cargo test -p ruscadb-graph
---

# SPEC-0041 — Algoritmos de grafo (BFS, componentes, PageRank)

## Contexto

`ruscadb-graph` solo tiene CSR + traversal. Se añade `algorithms.rs`:
`bfs`, `connected_components` (débilmente conexas) y `pagerank` (iteración de
potencia, Brin & Page 1998, damping 0.85, tolerancia configurable, manejo de
nodos colgantes). Determinista y sin panics.

## Criterios de aceptación

- **AC-0041-01/02** — BFS y componentes.
- **AC-0041-03/04** — PageRank (hub y dangling).
- **AC-0041-05** — fronteras.

## Trazabilidad

Tests `test_ac_0041_<nn>_*`; verificado por `cargo xtask trace`.
