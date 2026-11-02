---
id: SPEC-0185
feature: surreal_graph_relations
status: implemented
maturity: real
owner: query-team
appetite_days: 10
boundaries:
  crates: [ruscadb, ruscadb-query, ruscadb-graph]
  files:
    - crates/ruscadb-query/src/ast.rs
    - crates/ruscadb-query/src/parser.rs
    - crates/ruscadb/src/executor/knn.rs
    - crates/ruscadb/src/database.rs
  out_of_scope: [LIVE SELECT, permisos por campo, servidor TCP]
fr:
  - { id: FR-0185-01, desc: "RELATE a->edge->b crea/actualiza aristas con etiqueta" }
  - { id: FR-0185-02, desc: "navegacion SELECT ->label-> y <-label<- en proyeccion" }
nf:
  - { id: NF-0185-01, desc: "sin dependencias nuevas; embedded/0-red" }
acceptance_criteria:
  - id: AC-0185-01
    given: dos registros existentes y RELATE a->edge->b
    when: se ejecuta dos veces
    then: la primera crea la arista y la segunda devuelve affected=0 (idempotente)
    test: test_ac_0185_01_relate_idempotent_sql
  - id: AC-0185-02
    given: una arista knows entre dos personas
    when: se ejecuta SELECT ->knows->person
    then: se devuelve el RecordId del vecino con esa etiqueta
    test: test_ac_0185_02_graph_path_sql
  - id: AC-0185-03
    given: una cadena a->knows->b->knows->c
    when: se ejecuta un path de dos saltos
    then: se devuelve c resolviendo nivel a nivel (BFS)
    test: test_ac_0185_03_chained_path_sql
---

# SPEC-0185 — Relaciones y paths de grafo estilo SurrealDB (propuesta)

## Contexto

SurrealDB modela el grafo como una tabla de aristas (`in`/`out`/datos) y expone
`RELATE` y navegación `->label->` en `SELECT`. RuscaDB ya guarda
`Record.edges { label, node }` y un CSR, pero **no hay SQL** para crear aristas
ni para navegar por etiqueta: solo `TRAVERSE <col> DEPTH n`, que ignora labels.

## Diseño propuesto

1. `Statement::Relate { from: RecordRef, edge: String, to: RecordRef, content }`.
2. `RecordRef` = `tabla:id` (el `id` es el ULID actual).
3. Persistencia: insertar/actualizar la arista en los `edges.out` del origen y
   `edges.incoming` del destino; índice compuesto `(in,out,label)` para
   unicidad y búsqueda.
4. Navegación: `SELECT ... FROM t` con proyección `->label->tabla` y
   `<-label<-tabla`; BFS por niveles con `BTreeSet<RecordId>` visitados.
5. Rebobinar/rollback con la transacción MVCC activa (sin estado global).

## Complejidad

```text
RELATE:            O(log n) por arista (indice primario/compuesto)
navegacion BFS:    O(V + E) con dedupe por RecordId
materializar ruta: O(b^d)  -> prohibido; se poda por nivel
bidireccional:     O(b^(d/2)) si se conoce el destino
```

## Estado de implementación (2026-11-02)

- `Statement::Relate { from, edge, to }` + `Database::relate(...)`:
  persiste `edges.out`/`edges.incoming` en una sola transacción MVCC e
  idempotencia por `(label, node)` (sin contenido de arista, fuera de alcance).
- `GraphPath { hops, alias }` con uno o más saltos; cada salto filtra por
  etiqueta y tabla y deduplica por `RecordId` con `BTreeSet`.
- La navegación se resuelve **nivel a nivel** (BFS por saltos), no por producto
  de grados.
- `LIVE SELECT` sigue fuera de alcance (sin servidor).

## Criterios de aceptación

- `RELATE` crea exactamente una arista y es idempotente.
- `SELECT ->knows->person FROM person` devuelve solo vecinos con esa label.
- Un path encadenado resuelve BFS por niveles (sin producto de grados).

## Referencias

- SurrealDB, *RELATE* y *Graph model* (docs oficiales).
- BFS/DFS: Cormen et al., *Introduction to Algorithms* (4ª ed.).
