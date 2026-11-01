# SurrealDB vs RuscaDB — estado del arte y gaps (2026-11-02)

> Fuente primaria: documentación oficial de SurrealDB (`RELATE`, grafo,
> multi-modelo, tipos) y papers citados al final. El objetivo es decidir qué
> merece la pena portar a RQL sin romper la filosofía embedded/0-red de RuscaDB.

## 1. Modelo de SurrealDB (lo relevante)

- **Multi-modelo nativo**: documento, grafo, relacional, vector, time-series y
  geometría sobre el mismo registro; IDs `table:id`.
- **Grafo como tablas de aristas**: `RELATE a->edge->b` crea una fila
  `edge { in: a, out: b, ... }`; las aristas pueden tener datos y se consultan
  como tablas normales.
- **Navegación en SELECT**: `SELECT ->wrote->article FROM person:aristotle`
  expande el grafo en la proyección; `<-` recorre hacia atrás.
- **Record links/REFERENCES**: un campo puede referenciar otro registro; el
  grafo es explícito (tabla de aristas), no un puntero oculto.
- **SurrealQL**: `DEFINE TABLE/FIELD/INDEX`, `LET`, `IF ... ELSE`, `RETURN`,
  `LIVE SELECT`, `FETCH`, `OMIT`, `SPLIT`, `UPSERT`, `CREATE`, `MERGE`, `PATCH`,
  `RELATE`, `BEGIN/COMMIT/CANCEL`.
- **Permisos por tabla/campo** y `LIVE SELECT` (suscripción) asumen un servidor;
  en RuscaDB (embebida, sin red) `LIVE` no aplica y `DEFINE` debe mapearse a la
  API de catálogo existente.

## 2. Comparativa con RuscaDB

| Capacidad SurrealDB | RuscaDB hoy | Gap / decisión |
|---|---|---|
| `RELATE a->edge->b` | `Record.edges { label, node }` + CSR, sin SQL | **Gap**: statement + índice/actualización de aristas |
| `SELECT ->edge->table` | `TRAVERSE col DEPTH n` (sin etiqueta) | **Gap**: path por etiqueta y proyección de vecinos |
| `LET`/variables | no existe | **Gap**: entorno de ejecución por sesión |
| `IF/ELSE`, `RETURN` | `CASE`, `RETURNING` | Parcial; faltan `IF` y `RETURN` de expresión |
| `DEFINE` | `create_table`/`create_index` (API) | Mapear a RQL es posible; prioridad media |
| `LIVE SELECT` | no aplica (sin servidor) | **Fuera de alcance** (decisión) |
| `UPSERT`/`CREATE`/`MERGE`/`PATCH` | `INSERT ... ON CONFLICT`, `UPDATE SET` | **Gap** de azúcar/`MERGE` de documento |
| `FETCH`/`OMIT`/`SPLIT` | proyección/`->` documental | **Gap** de ergonomía |
| Vector `<|k|>` | `KNN ... USING` | **Paridad** (mejor: modos explícitos) |
| FTS/BM25 | `MATCH` + BM25 | **Paridad** |
| Graph algorithms | CSR + `TRAVERSE` | **Mejora**: BFS etiquetado O(V+E) |

## 3. Algoritmos y complejidad aplicables

### 3.1 Navegación de grafo

BFS/DFS con adyacencia etiquetada:

```text
O(V + E)   por traversal completo
O(b^d)     si se materializa cada camino (evitar; usar BFS por niveles)
O(b^(d/2)) bidirectional BFS (si se conoce el destino)
```

Regla: nunca expandir producto cartesiano de grados (`∏ deg(v_i)`); se poda por
nivel y se deduplica por `RecordId` con un `BTreeSet` (`O(log V)` por visita).

### 3.2 `RELATE`

Crear una arista es una inserción en la tabla de aristas + actualización de los
dos extremos: `O(log n)` con el índice primario; sin él, `O(n)` por escaneo.
La unicidad `(in, out, label)` se garantiza con un índice compuesto.

### 3.3 Joins y multi-modelo

Un join nested-loop es `O(|R|·|S|)`; hash-join `O(|R|+|S|)`; para multi-join,
WCOJ (Ngo et al.) garantiza `O(N^{ρ*})` con `ρ*` la cobertura fraccional de
aristas, frente a `O(N^{#relaciones})` del producto. La navegación de grafo es
un caso de join por `RecordId` y debe resolverse con índice, no con scan.

### 3.4 Vectores y texto

HNSW `O(log n)` esperado por consulta (Malkov & Yashunin); BM25
`O(postings)` por término. RuscaDB ya los implementa; el gap es exponerlos en
la misma sintaxis multi-modelo de SurrealQL.

## 4. Decisiones propuestas (SDD)

1. **SPEC-0185** `RELATE` + paths etiquetados (`->label->`, `<-label<-`), BFS
   O(V+E), índice compuesto `(in,out,label)`.
2. **SPEC-0186** variables de sesión (`LET`) e `IF/ELSE`/`RETURN` de expresión:
   entorno `BTreeMap` por query, sin estado global.
3. **Fuera de alcance**: `LIVE SELECT` (requiere servidor/red), permisos de
   campo con `DEFINE` (se mantiene la API de catálogo/RBAC actual).

## 5. Referencias

- SurrealDB. *RELATE* y *Graph model* (documentación oficial).
  https://surrealdb.com/docs/surrealql/statements/relate
- SurrealDB. *Data types / multi-model*.
- Ngo, H. Q., Porat, E., Ré, C., Rudra, A. (2018). Worst-Case Optimal Join
  Algorithms. *PODS*.
- Malkov, Y., Yashunin, D. (2020). Efficient and robust ANN search using HNSW.
  *IEEE TPAMI*.
- Robertson, S., Zaragoza, H. (2009). The Probabilistic Relevance Framework:
  BM25 and Beyond. *FnTIR*.
- O'Neil, P. et al. (1996). The Log-Structured Merge-Tree (LSM-Tree). *Acta
  Informatica*.
- Kleene, S. C. (1952). *Introduction to Metamathematics* (3VL).
