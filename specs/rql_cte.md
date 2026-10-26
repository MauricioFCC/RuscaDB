---
id: SPEC-0163
feature: rql_cte
status: implemented
owner: query-team
appetite_days: 7
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/executor.rs]
  out_of_scope: [materialized CTE hints, recursive CTE optimization]
fr:
  - { id: FR-0163-01, desc: "WITH name AS (query) SELECT ..." }
  - { id: FR-0163-02, desc: "multiples CTEs encadenados" }
  - { id: FR-0163-03, desc: "WITH RECURSIVE para grafos/arboles" }
nf:
  - { id: NF-0163-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0163-01
    given: una consulta con WITH ... AS (subquery)
    when: se ejecuta
    then: el CTE se materializa y se consulta
    test: test_ac_0163_01_cte
  - id: AC-0163-02
    given: una consulta con WITH RECURSIVE
    when: se ejecuta sobre una jerarquia
    then: se devuelven todos los nodos alcanzables
    test: test_ac_0163_02_recursive
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-query --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0163 — CTE (WITH) y CTE recursivo

## Contexto

Los CTEs mejoran legibilidad y permiten consultas recursivas (grafos,
jerarquias). RuscaDB no los soporta.

## Superficie

- `ast.rs`: `Statement::With { ctes: Vec<Cte>, body: Box<Select> }`,
  `Cte { name, columns, query, recursive }`.
- `parser.rs`: parsing de `WITH [RECURSIVE]`.
- `executor.rs`: materializacion en RowSet temporal + evaluacion recursiva
  con deteccion de ciclos.

## Estado del arte

- **SQL:1999**: WITH (CTE no recursivo).
- **SQL:1999 recursive**: WITH RECURSIVE.
- **PostgreSQL**: CTE materializado por defecto (hasta PG12), inlined desde PG12.
- **SQLite**: WITH RECURSIVE con deteccion de ciclos opcional.
- **RuscaDB actual**: sin CTE.

## Diseño propuesto

1. **No recursivo**: ejecutar cada CTE y registrar como tabla temporal en el
   catalogo de la sesion (scope de la consulta).
2. **Recursivo**: iterar hasta punto fijo (no nuevas filas) con limite de
   profundidad configurable (default 1000) para evitar loops infinitos.

## Riesgos

- Explosion combinatoria en CTE recursivo: limite de filas + timeout.

## ADR requerido

`docs/adr/ADR-0163-rql-cte.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\rql_cte.md