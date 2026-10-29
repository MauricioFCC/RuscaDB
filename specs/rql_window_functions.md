---
id: SPEC-0165
feature: rql_window_functions
status: implemented
maturity: stub
owner: query-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/executor.rs]
  out_of_scope: [frame EXCLUDE, GROUPS, FILTER]
fr:
  - { id: FR-0165-01, desc: "ROW_NUMBER, RANK, DENSE_RANK, NTILE" }
  - { id: FR-0165-02, desc: "LAG, LEAD, FIRST_VALUE, LAST_VALUE, NTH_VALUE" }
  - { id: FR-0165-03, desc: "SUM/AVG/COUNT/MIN/MAX OVER (window aggregates)" }
  - { id: FR-0165-04, desc: "PARTITION BY, ORDER BY, frame ROWS/RANGE" }
nf:
  - { id: NF-0165-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0165-01
    given: SELECT con ROW_NUMBER() OVER (ORDER BY col)
    when: se ejecuta
    then: se asigna numero secuencial por orden
    test: test_ac_0165_01_row_number
  - id: AC-0165-02
    given: SELECT con SUM(col) OVER (PARTITION BY g)
    when: se ejecuta
    then: se devuelve la suma por particion
    test: test_ac_0165_02_sum_over
  - id: AC-0165-03
    given: SELECT con LAG(col, 1)
    when: se ejecuta
    then: se devuelve el valor de la fila anterior
    test: test_ac_0165_03_lag
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-query --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0165 — Window functions reales (OVER)

## Contexto

RuscaDB tiene un stub `WindowFunc` (row_number/rank) sin integracion con el
parser ni el executor. Esta spec implementa window functions completas.

## Superficie

- `ast.rs`: `Expr::Window { func, over: WindowSpec }`,
  `WindowSpec { partition_by, order_by, frame }`.
- `parser.rs`: `func() OVER (...)`.
- `executor.rs`: evaluacion en dos pasos (particionar + ordenar + frame).

## Estado del arte

- **SQL:2003**: window functions.
- **PostgreSQL**: implementacion completa con frames.
- **DuckDB**: vectorized window functions.
- **RuscaDB actual**: stub `WindowFunc` (SPEC-0141) sin parser/executor.

## Diseño propuesto

1. **Partition**: agrupar filas por partition_by.
2. **Order**: ordenar cada particion por order_by.
3. **Frame**: para cada fila, aplicar el agregado sobre el frame (ROWS/RANGE).
4. **Funciones**: row_number, rank, dense_rank, ntile, lag, lead, first/last/
   nth_value, y agregados (sum/avg/count/min/max) como window.

## Riesgos

- Memoria: materializar todas las particiones; aceptable para embedded.

## ADR requerido

`docs/adr/ADR-0165-rql-window-functions.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\rql_window_functions.md