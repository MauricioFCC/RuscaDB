---
id: SPEC-0017
feature: executor_integration
status: accepted
owner: query-team
appetite_days: 10
boundaries:
  crates: [ruscadb, ruscadb-query, ruscadb-fts, ruscadb-vector, ruscadb-graph, ruscadb-core]
  out_of_scope: [MVCC, manifiesto, DataFusion, persistencia de indices, iFVS]
fr:
  - { id: FR-0017-01, desc: "parsear MATCH(col, 'texto') como predicado de WHERE (FTS)" }
  - { id: FR-0017-02, desc: "indices por tabla en la fachada: HNSW (vector), CSR (grafo), invertido (FTS), actualizados en insert" }
  - { id: FR-0017-03, desc: "insert_record persiste el Record completo (scalars+vector+edges) y alimenta los indices" }
  - { id: FR-0017-04, desc: "executor resuelve KNN (vector) y TRAVERSE (grafo) ademas de SELECT/MATCH" }
  - { id: FR-0017-05, desc: "errores accionables: columna sin vector/grafo, tabla ausente, dimension incompatible" }
nf:
  - { id: NF-0017-01, desc: "KNN usa el indice HNSW (no fuerza bruta) y respeta k" }
  - { id: NF-0017-02, desc: "sin panics; WHERE/LIMIT/proyeccion combinables con las nuevas clausulas" }
acceptance_criteria:
  - id: AC-0017-01
    given: una tabla con documentos de texto
    when: se ejecuta "SELECT * FROM t WHERE MATCH(titulo, 'gato')"
    then: devuelve las filas cuyo titulo contiene el termino, ordenadas por BM25
    test: test_ac_0017_01_match_full_text_end_to_end
  - id: AC-0017-02
    given: una tabla con registros con embedding
    when: se ejecuta "SELECT * FROM t KNN embedding <|2|> [0.1, 0.2, 0.3]"
    then: devuelve los 2 vecinos mas cercanos por el indice HNSW
    test: test_ac_0017_02_knn_vector_end_to_end
  - id: AC-0017-03
    given: una tabla con registros y aristas
    when: se ejecuta "SELECT * FROM t TRAVERSE edges DEPTH 2"
    then: devuelve los nodos alcanzables hasta profundidad 2
    test: test_ac_0017_03_traverse_graph_end_to_end
  - id: AC-0017-04
    given: una tabla con datos
    when: se combinan WHERE + KNN/TRAVERSE + LIMIT + proyeccion
    then: se aplican en orden y el resultado respeta todas las clausulas
    test: test_ac_0017_04_combined_clauses
  - id: AC-0017-05
    given: una tabla sin columna vectorial o una tabla inexistente
    when: se ejecuta KNN/TRAVERSE/MATCH
    then: devuelve un error accionable (sin panics)
    test: test_ac_0017_05_integration_errors_are_actionable
  - id: AC-0017-06
    given: una base con filas e índices (secundario, HNSW, CSR, FTS)
    when: se cierra y reabre
    then: los índices se reconstruyen y las consultas devuelven lo mismo
    test: test_ac_0017_06_indexes_survive_reopen
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0017
  - cargo mutants -p ruscadb -p ruscadb-query mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir executor/indices a la version SPEC-0012
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0017 — Integración end-to-end (FTS/KNN/TRAVERSE en el executor)

## Contexto

Cierra la ruta vertical de los tres modelos de búsqueda sobre la fachada
`ruscadb`: el parser ya produce `KNN`/`TRAVERSE` (SPEC-0015) y existe el crate
`ruscadb-fts` (SPEC-0014); falta (1) `MATCH` en el `WHERE`, (2) índices por
tabla mantenidos en la fachada y (3) la resolución en el executor.

- `MATCH(col, 'texto')` → nuevo `Expr::Match` en `ruscadb-query`.
- La fachada mantiene por tabla: `ruscadb-vector::HnswIndex`,
  `ruscadb-graph::CsrGraph` y `ruscadb-fts::InvertedIndex`, actualizados en
  cada `insert_record`.
- `insert_record(table, Record)` persiste el registro completo (escalares +
  `vector` + `edges`) y alimenta los índices; `insert(table, scalars)` sigue
  existiendo (delegación).
- El executor combina `WHERE` (incl. `MATCH`), `KNN`, `TRAVERSE`, `LIMIT` y
  proyección.

## Criterios de aceptación

- **AC-0017-01..03** — MATCH/KNN/TRAVERSE end-to-end.
- **AC-0017-04** — combinación de cláusulas.
- **AC-0017-05** — errores accionables.

## Trazabilidad

Tests `test_ac_0017_<nn>_*` en `ruscadb`/`ruscadb-query`; `cargo xtask trace`.
