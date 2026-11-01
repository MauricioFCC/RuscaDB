---
id: SPEC-0185
feature: surreal_graph_relations
status: proposed
maturity: stub
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

## Criterios de aceptación (pendientes de implementar)

- `RELATE` crea exactamente una arista y es idempotente con índice único.
- `SELECT ->knows->person FROM person:a` devuelve solo vecinos con esa label.
- Traversal con 10k nodos no explota el producto de grados (BFS O(V+E)).
- `LIVE SELECT` queda fuera de alcance (sin servidor).

## Referencias

- SurrealDB, *RELATE* y *Graph model* (docs oficiales).
- BFS/DFS: Cormen et al., *Introduction to Algorithms* (4ª ed.).
