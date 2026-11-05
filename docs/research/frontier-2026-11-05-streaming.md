# Frontera: streaming de resultados (2026-11-05)

> Cierre del gap "sin cursores ni streaming". `Database::execute` materializa
> `Vec<Row>`; se añade streaming memoria O(1) para `SELECT` simples (SPEC-0210).

## 1. Research SOTA — fuentes leídas

| Fuente | URL | Aporte |
|---|---|---|
| PostgreSQL 18 — `DECLARE` | https://www.postgresql.org/docs/current/sql-declare.html | cursor server-side, `FETCH`, `WITH HOLD`; el cliente no recibe todo el resultado |
| DuckDB Python — results | https://duckdb.org/docs/stable/clients/python/overview.html | `fetchmany`/`fetch_df_chunk` y ejecución vectorizada por lotes |
| Apache Arrow / DataFusion | https://arrow.apache.org/docs/ | `RecordBatch` como unidad de streaming; operadores pull/push |
| SQLite `sqlite3_step` | https://sqlite.org/c3ref/step.html | iteración fila a fila sobre una sentencia preparada |
| Rust `Iterator`/callbacks | https://doc.rust-lang.org/std/iter/ | composición perezosa; el consumidor decide cuándo parar |

## 2. Hipótesis comparadas

| # | Estrategia | Memoria | Cobertura | Coste/riesgo |
|---|---|---|---|---|
| H1 | Cursores reanudables (`DECLARE`/`FETCH`) | O(1) | total | alto: estado de cursor, MVCC, reconexión |
| H2 | Callback por fila sobre scan simple (elegida) | O(1) | SELECT simple | bajo: reutiliza `is_visible`/`keeps_row`/`project_row` |
| H3 | Por lotes (`RecordBatch`/`fetchmany`) | O(lote) | total | medio: nuevo tipo de lote y buffering |
| H4 | Materializar y devolver `Vec` (actual) | O(R) | total | ninguno, pero no resuelve el gap |

**Decisión:** H2. Máximo ROI sin introducir cursores reanudables ni un nuevo
tipo de lote. El callback `FnMut(&Row) -> Result<bool, RuscaError>` permite
parada temprana y mantiene una sola fila viva. Las formas que requieren estado
global (JOIN, agregados, ORDER BY, DISTINCT, KNN, TRAVERSE, ventanas,
subqueries derivadas) se rechazan con error accionable a `execute()`.

## 3. Diseño

```text
execute_stream(q, visit):
  parse SELECT simple -> rechazar formas materializantes
  snapshot MVCC; Catalog::load; materializar subqueries del WHERE (una vez)
  heap_for_each(table) -> [is_visible && keeps_row] -> OFFSET -> project_row
                        -> visit(&row)?  (false corta)
  respeta LIMIT + IMPLICIT_LIMIT + max_result_rows; cuenta filas entregadas
```

`heap_for_each` comparte con `heap_scan` el parseo de slots y la reconstrucción
de overflow, pero entrega por callback en vez de acumular en `Vec`.

Complejidad: `execute` O(N) tiempo / O(R) memoria; `execute_stream` O(N)
tiempo / O(1) memoria (más lo que ocupen las subconsultas materializadas).
Parada temprana: O(filas visitadas hasta el corte).

## 4. Gap analysis

| Gap | Acción |
|---|---|
| Resultado siempre materializado | callback por fila con parada temprana |
| `heap_scan` acoplado a `Vec` | `heap_for_each` + helper de página compartido |
| Cliente no puede procesar y descartar | callback recibe `&Row` |
| Formas complejas silenciosamente caras | rechazo explícito con `execute()` |

## 5. No aplicado

- **Cursores reanudables** (`DECLARE`/`FETCH`, `WITH HOLD`): requieren estado de
  sesión y política MVCC de larga duración.
- **Streaming de JOIN/agregados/ORDER BY/KNN/TRAVERSE/ventanas**: necesitan
  estado global o índices auxiliares; se materializan vía `execute()`.
- **Spill de hash join**: gap separado; requiere I/O temporal y formato de
  particiones.
- **Backpressure asíncrono / `Stream` de Rust**: el callback síncrono basta
  para el caso de uso actual; un adaptador async puede envolverlo después.
