---
id: SPEC-0161
feature: rql_expression_language
status: implemented
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/lexer.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/executor.rs]
  out_of_scope: [full SQL-92 compliance, window functions, subqueries]
fr:
  - { id: FR-0161-01, desc: "OR logico, NOT, IN, BETWEEN, LIKE, IS NULL en Expr" }
  - { id: FR-0161-02, desc: "CASE WHEN ... THEN ... ELSE ... END" }
  - { id: FR-0161-03, desc: "COALESCE, NULLIF, CAST, CONCAT (||)" }
  - { id: FR-0161-04, desc: "precedencia y asociatividad documentadas en el parser" }
nf:
  - { id: NF-0161-01, desc: "cero dependencias nuevas" }
  - { id: NF-0161-02, desc: "0 regresiones en los tests de ruscadb" }
acceptance_criteria:
  - id: AC-0161-01
    given: una consulta con OR/AND/NOT combinados
    when: se ejecuta
    then: el resultado respeta la precedencia (NOT > AND > OR)
    test: test_ac_0161_01_precedence
  - id: AC-0161-02
    given: una consulta con IN (lista de literales)
    when: se ejecuta
    then: se devuelven las filas cuyo valor pertenece a la lista
    test: test_ac_0161_02_in_list
  - id: AC-0161-03
    given: una consulta con BETWEEN a AND b
    when: se ejecuta
    then: se devuelven las filas en rango inclusivo
    test: test_ac_0161_03_between
  - id: AC-0161-04
    given: una consulta con CASE WHEN
    when: se ejecuta
    then: se evalua la primera rama verdadera
    test: test_ac_0161_04_case
  - id: AC-0161-05
    given: una consulta con COALESCE y NULLIF
    when: se ejecuta
    then: se devuelve el primer no-nulo / NULL si iguales
    test: test_ac_0161_05_coalesce
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-query --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0161 — Lenguaje de expresiones RQL completo

## Contexto

RuscaDB hoy solo soporta `AND` en `Expr` (sin OR/NOT/IN/BETWEEN/LIKE/CASE/
COALESCE). Esto limita cualquier consulta real. Esta spec extiende el AST y
el parser para cubrir el nucleo de SQL-92 expresiones.

## Superficie

- `crates/ruscadb-query/src/ast.rs`: `Expr` (Or, Not, In, Between, Like, Case,
  Coalesce, Nullif, Cast, Concat), `CompareOp` (Is/IsNot).
- `crates/ruscadb-query/src/lexer.rs`: tokens `OR`, `NOT`, `IN`, `BETWEEN`,
  `LIKE`, `ILIKE`, `CASE`, `WHEN`, `THEN`, `ELSE`, `END`, `IS`, `NULL`,
  `COALESCE`, `NULLIF`, `CAST`, `AS`.
- `crates/ruscadb-query/src/parser.rs`: precedencia documentada.
- `crates/ruscadb/src/executor.rs`: evaluacion de las nuevas expresiones.

## Estado del arte

- **SQL-92** (ANSI): expresiones booleanas con precedencia NOT > AND > OR.
- **SQLite**: `expr` grammar con `OR`/`AND`/`NOT`/`BETWEEN`/`IN`/`LIKE`/
  `GLOB`/`REGEXP`/`MATCH`/`IS`/`CASE`/`CAST`/`COALESCE`/`NULLIF`.
- **DuckDB**: expresiones vectorizadas sobre columnas.
- **RuscaDB actual**: solo `And` + `Compare` + `Column`/literales + `Match`
  + `DocExtract`/`DocContains`.

## Diseño propuesto

1. **AST**: extender `Expr` con variantes `Or`, `Not`, `In { expr, list }`,
   `Between { expr, low, high }`, `Like { expr, pattern, case_insensitive }`,
   `Case { whens, else_ }`, `Coalesce(Vec<Expr>)`, `Nullif(a,b)`,
   `Cast { expr, ty }`, `Concat(Vec<Expr>)`, `IsNull(Box<Expr>)`.
2. **Lexer**: keywords case-insensitive (ya lo es).
3. **Parser**: recursive descent con precedencia explicita (NOT > AND > OR).
4. **Executor**: evaluacion NULL-semantica (three-valued logic) para IS NULL.

## Alternativas descartadas

- Adoptar `sqlparser-rs`: mas completo pero anade dep y pierde control del AST;
  se mantiene hand-written por ADR-013.

## Riesgos

- NULL-semantics: definir claramente (SQL standard: NULL en comparaciones -> NULL).
- Performance del parser con expresiones profundas: limitar profundidad (limits.rs).

## ADR requerido

`docs/adr/ADR-0161-rql-expressions.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\rql_expression_language.md