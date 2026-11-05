---
id: SPEC-0206
feature: analyze_rql
status: implemented
maturity: real
owner: query-team
appetite_days: 1
boundaries:
  crates: [ruscadb-query, ruscadb, ruscadb-wasm]
  files:
    - crates/ruscadb-query/src/lexer.rs
    - crates/ruscadb-query/src/ast.rs
    - crates/ruscadb-query/src/parser.rs
    - crates/ruscadb/src/table_api.rs
    - crates/ruscadb/tests/analyze_rql.rs
  out_of_scope: [ANALYZE de columnas, ANALYZE incremental, muestreo]
fr:
  - { id: FR-0206-01, desc: "RQL acepta ANALYZE <tabla> y refresca las estadisticas" }
  - { id: FR-0206-02, desc: "AST/Display/WASM conocen la sentencia; tabla inexistente falla" }
nf:
  - { id: NF-0206-01, desc: "requiere modo write y rol Write; devuelve 0 filas" }
  - { id: NF-0206-02, desc: "sin cambio de formato persistido; reusa Database::analyze" }
acceptance_criteria:
  - id: AC-0206-01
    given: una tabla con filas
    when: se ejecuta ANALYZE t
    then: las estadisticas quedan frescas y no se devuelven filas
    test: test_ac_0206_01_analyze_rql_refreshes_stats
  - id: AC-0206-02
    given: el texto ANALYZE t o una tabla inexistente
    when: se parsea o ejecuta
    then: el AST/Display es estable y la tabla inexistente devuelve error
    test: test_ac_0206_02_analyze_parser_and_errors
exit_criteria:
  - cargo test -p ruscadb --test analyze_rql
  - cargo test -p ruscadb-query
  - cargo test -p ruscadb-wasm
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar la sentencia del parser; la API Database::analyze sigue disponible
sandbox:
  - cargo test -p ruscadb --test analyze_rql
---
# SPEC-0206 — ANALYZE en RQL

## Contexto

SPEC-0201 expuso `Database::analyze` solo por API; un cliente SQL no podía
refrescar estadísticas. SQLite/PostgreSQL tienen `ANALYZE` como sentencia.

## Diseño

1. Keyword `ANALYZE` y `Statement::Analyze { table }`.
2. Parser: `ANALYZE <identificador>`.
3. Executor: `Database::analyze(table)` (write) y `Vec::new()`.
4. WASM serializa `{"kind":"analyze","table":...}`.
5. `is_write_statement` lo marca como escritura.

## Complejidad

```text
parse: O(|SQL|); analyze: O(N); memoria O(C·B)
```

## TDD adversarial

`ANALYZE t` refresca stats y devuelve 0 filas; AST/Display estables; tabla
inexistente devuelve error; WASM parsea `kind=analyze`.

## Riesgos

`ANALYZE` con columnas o incremental fuera de alcance; coste O(N) explícito.
