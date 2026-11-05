---
id: SPEC-0210
feature: result_streaming
status: implemented
maturity: real
owner: query-team
appetite_days: 2
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/heap.rs
    - crates/ruscadb/src/table_api.rs
    - crates/ruscadb/tests/result_streaming.rs
  out_of_scope: [spill de hash join, cursores reanudables, streaming de JOIN/agregados/ORDER BY/KNN/TRAVERSE, backpressure asincrono]
fr:
  - { id: FR-0210-01, desc: "Database::execute_stream entrega filas por callback sin materializar el resultado" }
  - { id: FR-0210-02, desc: "el callback puede cortar el recorrido devolviendo false (parada temprana)" }
  - { id: FR-0210-03, desc: "las formas que requieren materializar se rechazan con un error accionable a execute()" }
  - { id: FR-0210-04, desc: "respeta max_result_rows, el deadline del query budget y propaga errores del callback" }
nf:
  - { id: NF-0210-01, desc: "memoria O(1) en filas durante el scan; una fila viva a la vez" }
  - { id: NF-0210-02, desc: "mismas filas que execute para SELECT simple (filtro, proyeccion, OFFSET y LIMIT)" }
  - { id: NF-0210-03, desc: "respeta visibilidad MVCC y max_result_rows; sin cache semantica ni de resultados" }
  - { id: NF-0210-04, desc: "el deadline se comprueba por fila visitada (QueryTimeout) y cada llamada toma un snapshot nuevo" }
acceptance_criteria:
  - id: AC-0210-01
    given: una tabla con filas y un SELECT simple con WHERE/OFFSET/LIMIT
    when: se ejecuta con execute_stream
    then: entrega exactamente las mismas filas que execute
    test: test_ac_0210_01_stream_matches_execute
  - id: AC-0210-02
    given: un SELECT simple y un callback que deja de aceptar tras N filas
    when: se ejecuta con execute_stream
    then: el recorrido se corta y devuelve N filas entregadas
    test: test_ac_0210_02_stream_early_stop
  - id: AC-0210-03
    given: un SELECT con ORDER BY/DISTINCT/agregados/JOIN/MATCH o una sentencia DML
    when: se intenta ejecutar con execute_stream
    then: devuelve TypeMismatch accionable que remite a execute()
    test: test_ac_0210_03_stream_rejects_materializing
  - id: AC-0210-04
    given: un callback que falla, una tabla vacia y un tope max_result_rows
    when: se ejecuta execute_stream
    then: el error se propaga, la tabla vacia entrega 0 y el tope recorta el conteo
    test: test_ac_0210_04_stream_robustness
exit_criteria:
  - cargo test -p ruscadb --test result_streaming
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar execute_stream y heap_for_each; execute sigue materializando como antes
sandbox:
  - cargo test -p ruscadb --test result_streaming -- --nocapture
---
# SPEC-0210 — Streaming de resultados

## Contexto

`Database::execute` materializa siempre `Vec<Row>`: un `SELECT` sobre una tabla
grande reserva memoria proporcional al número de filas y no permite al cliente
procesar y descartar filas. El gap estaba declarado como "sin cursores ni
streaming". Se cubre el caso de mayor ROI (SELECT simple) sin introducir
cursores reanudables ni spill.

## Diseño

1. `heap_for_each(database, table, visit)` recorre las páginas del heap y
   entrega `(RowLocator, Record)` por callback; `false` corta el recorrido.
   `collect_page_rows` se refactoriza sobre el mismo helper, sin duplicar el
   parseo de slots ni la reconstrucción de overflow (SPEC-0074).
2. `Database::execute_stream(query, visit)` parsea el `SELECT`, rechaza las
   formas materializantes y, para las simples:
   - materializa una sola vez las subconsultas del `WHERE` (no correlacionadas),
   - toma un snapshot MVCC y carga el catálogo/tabla,
   - recorre el heap aplicando `is_visible`, `keeps_row`, `OFFSET`, proyección y
     `LIMIT` (incluye `IMPLICIT_LIMIT` y `max_result_rows`),
   - entrega cada `Row` al callback y corta si devuelve `false`.
3. No usa la caché semántica ni la de planes: el resultado no se materializa.
4. Se rechazan con `TypeMismatch` accionable (remite a `execute()`): `JOIN`,
   tabla derivada, `MATCH` full-text, `KNN`, `TRAVERSE`,
   agregados/`GROUP BY`/`HAVING`, `ORDER BY`, `DISTINCT`, ventanas,
   expresiones y paths.

## Complejidad

```text
execute:        tiempo O(N) · memoria O(R)   (R filas devueltas)
execute_stream: tiempo O(N) · memoria O(1)   (una fila viva; + O(subconsultas))
parada temprana: tiempo O(filas visitadas hasta el corte)
```

## TDD adversarial

- Paridad exacta con `execute` en filtro, proyección, `OFFSET` y `LIMIT`,
  incluido `LIMIT 0`.
- Parada temprana: el callback corta y el conteo coincide con lo entregado.
- Rechazo de `ORDER BY`, `DISTINCT`, agregados, `JOIN` y DML con mensaje que
  remite a `execute()`; las consultas siguen funcionando por el camino
  materializado.
- Robustez: error del callback propagado, tabla vacía sin invocar el callback,
  snapshot fresco por llamada y tope `max_result_rows` aplicado al streaming.

## Riesgos

- El orden de las filas es el de inserción (scan completo); sin `ORDER BY` no
  hay garantía de orden, igual que en `execute`.
- No hay cursores reanudables, backpressure asíncrono ni streaming de
  agregados/JOIN/ORDER BY/KNN/TRAVERSE; esas formas se materializan o se
  rechazan explícitamente.
- El spill de hash join queda como gap separado (requiere I/O temporal).
