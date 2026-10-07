---
id: SPEC-0007
feature: graph_store
status: accepted
owner: index-team
appetite_days: 6
boundaries:
  crates: [ruscadb-graph, ruscadb-core]
  out_of_scope: [storage, wal, query]
fr:
  - { id: FR-0007-01, desc: "grafo en formato CSR (offsets + vecinos) para salientes y entrantes" }
  - { id: FR-0007-02, desc: "traversal BFS acotado por profundidad y numero maximo de nodos" }
  - { id: FR-0007-03, desc: "neighbors(node, direction) devuelve vecinos directos" }
nf:
  - { id: NF-0007-01, desc: "neighbors O(grado); traversal O(V+E) acotado" }
  - { id: NF-0007-02, desc: "sin panics ante nodos desconocidos" }
acceptance_criteria:
  - id: AC-0007-01
    given: un grafo con aristas 1->2 y 1->3
    when: se piden los vecinos salientes de 1
    then: devuelve {2,3} (ordenados)
    test: test_ac_0007_01_add_edge_and_neighbors
  - id: AC-0007-02
    given: una cadena 1->2->3->4
    when: se hace BFS desde 1 con profundidad 1
    then: devuelve {1,2} y no incluye 3 ni 4
    test: test_ac_0007_02_bfs_respects_depth
  - id: AC-0007-03
    given: un grafo con muchos nodos alcanzables
    when: se acota max_nodes
    then: el resultado no supera max_nodes
    test: test_ac_0007_03_max_nodes_limit
  - id: AC-0007-04
    given: un nodo inexistente
    when: se piden sus vecinos o se traversa desde el
    then: devuelve vacio (sin panic)
    test: test_ac_0007_04_unknown_node_is_empty
exit_criteria:
  - cargo test -p ruscadb-graph -- test_ac_0007
  - cargo mutants -p ruscadb-graph mutation score >= 70%
rollback:
  - revertir ruscadb-graph al stub
sandbox:
  - cargo test -p ruscadb-graph
---

# SPEC-0007 — Almacén de grafo (CSR + traversal)

## Contexto

Modelo de grafo de RuscaDB (`docs/RuscaDB-roadmap.md` §5.4). Representación
**CSR** (Compressed Sparse Row) para adyacencia saliente/entrante y traversal
BFS acotado (protección de RAM).

## API esperada

`NodeId = u64`; `Direction { Out, In, Both }`; `CsrGraph` con
`add_edge`, `build`, `neighbors`, `traverse`, `node_count`, `edge_count`.

## Trazabilidad

Tests `test_ac_0007_<nn>_*`; verificado por `cargo xtask trace`.
