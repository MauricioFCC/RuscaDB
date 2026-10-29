---
id: SPEC-0162
feature: rql_subqueries
status: implemented
maturity: stub
owner: query-team
appetite_days: 7
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/executor.rs]
  out_of_scope: [correlated subqueries, lateral joins]
fr:
  - { id: FR-0162-01, desc: "subconsulta escalar en SELECT/proyeccion" }
  - { id: FR-0162-02, desc: "subconsulta en WHERE con IN (subquery)" }
  - { id: FR-0162-03, desc: "EXISTS / NOT EXISTS" }
  - { id: FR-0162-04, desc: "tabla derivada en FROM (subquery en FROM)" }
nf:
  - { id: NF-0162-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0162-01
    given: SELECT con subconsulta escalar
    when: se ejecuta
    then: se devuelve el valor calculado por la subconsulta
    test: test_ac_0162_01_scalar
  - id: AC-0162-02
    given: WHERE col IN (subquery)
    when: se ejecuta
    then: se filtran filas cuyo valor esta en el resultado
    test: test_ac_0162_02_in
  - id: AC-0162-03
    given: WHERE EXISTS (subquery)
    when: se ejecuta
    then: se devuelven las filas donde la subconsulta es no-vacia
    test: test_ac_0162_03_exists
  - id: AC-0162-04
    given: FROM (subquery) AS alias
    when: se ejecuta
    then: se consulta la tabla derivada
    test: test_ac_0162_04_derived
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-query --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0162 — Subconsultas (scalar, IN, EXISTS, derived tables)

## Contexto

Sin subconsultas, RQL no puede expresar consultas anidadas ni filtros
dependientes. Es el bloqueante principal para compatibilidad con SQL.

## Superficie

- `ast.rs`: `Expr::Subquery(Box<Select>)`, `Expr::Exists(Box<Select>)`,
  `FromClause::Subquery { select, alias }`.
- `parser.rs`: parsing de parentesis que contienen SELECT.
- `executor.rs`: ejecucion recursiva con snapshot del estado actual.

## Estado del arte

- **SQL-92**: subconsultas escalares, predicados, tablas derivadas.
- **SQLite**: soporta todas las formas; optimiza con flattening.
- **DuckDB**: subquery unnesting + decorrelation.
- **RuscaDB actual**: sin subconsultas.

## Diseño propuesto

1. **Scalar subquery**: ejecutar y tomar la primera fila/columna.
2. **IN (subquery)**: materializar el conjunto y testear pertenencia.
3. **EXISTS**: ejecutar y verificar no-vacio (short-circuit).
4. **Derived table**: ejecutar la subconsulta y tratar el resultado como tabla
   temporal en memoria (RowSet).

## Alternativas descartadas

- Flattening automatico (optimizacion): se deja para el optimizador.

## Riesgos

- Recursion profunda: limitar profundidad (limits.rs).
- Correlacion: fuera de alcance en esta spec (separar).

## ADR requerido

`docs/adr/ADR-0162-rql-subqueries.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\rql_subqueries.md