---
id: SPEC-0209
feature: ddl_rql
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
    - crates/ruscadb-wasm/src/lib.rs
    - crates/ruscadb/tests/ddl_rql.rs
  out_of_scope: [DROP INDEX por nombre global, CASCADE, reclamacion de paginas]
fr:
  - { id: FR-0209-01, desc: "RQL acepta DROP INDEX tabla.columna y DROP TABLE tabla" }
  - { id: FR-0209-02, desc: "ejecuta drop_index/drop_table (write), idempotente y con AST/Display/WASM" }
nf:
  - { id: NF-0209-01, desc: "sin cambio de formato; requiere modo write y rol Write" }
  - { id: NF-0209-02, desc: "errores de sintaxis accionables si falta INDEX/TABLE o el punto" }
acceptance_criteria:
  - id: AC-0209-01
    given: una tabla con indice secundario
    when: se ejecuta DROP INDEX t.k
    then: el indice desaparece, EXPLAIN usa FullScan y repetir no falla
    test: test_ac_0209_01_drop_index_rql
  - id: AC-0209-02
    given: una tabla existente
    when: se ejecuta DROP TABLE t
    then: la tabla desaparece, las consultas fallan y el AST/Display es estable
    test: test_ac_0209_02_drop_table_rql
exit_criteria:
  - cargo test -p ruscadb --test ddl_rql
  - cargo test -p ruscadb-query
  - cargo test -p ruscadb-wasm
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar las sentencias del parser; las APIs drop_index/drop_table siguen
sandbox:
  - cargo test -p ruscadb --test ddl_rql
---
# SPEC-0209 — DDL en RQL (DROP INDEX / DROP TABLE)

## Contexto

SPEC-0205/0207 añadieron `drop_index`/`drop_table` por API, pero un cliente SQL
no podía eliminar índices ni tablas. PostgreSQL expone `DROP INDEX` y
`DROP TABLE`; aquí se añaden al parser reutilizando las APIs.

## Diseño

1. Keywords `DROP`/`INDEX`/`TABLE`.
2. AST `DropIndex { table, column }` y `DropTable { table }`.
3. Parser: `DROP INDEX tabla.columna` y `DROP TABLE tabla`.
4. Executor: `Database::drop_index`/`drop_table`, devuelve 0 filas.
5. WASM serializa `kind=drop_index|drop_table`.
6. `is_write_statement` los marca como escritura.

## Complejidad

```text
parse: O(|SQL|); drop: O(I)/O(log T) según SPEC-0205/0207
memoria: O(1)
```

## TDD adversarial

- `DROP INDEX t.k` ⇒ FullScan; repetir no falla.
- `DROP TABLE t` ⇒ consultas fallan; AST/Display estables.
- Sintaxis incompleta (`DROP`, `DROP INDEX t`) devuelve error accionable.

## Riesgos

Sin `CASCADE` ni reclamación de páginas; `DROP INDEX` identifica por
`tabla.columna` (no hay nombres globales de índice).
