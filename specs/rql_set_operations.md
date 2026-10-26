---
id: SPEC-0164
feature: rql_set_operations
status: implemented
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/executor.rs]
  out_of_scope: [ALL/DISTINCT ordering semantics beyond standard]
fr:
  - { id: FR-0164-01, desc: "UNION y UNION ALL" }
  - { id: FR-0164-02, desc: "INTERSECT y INTERSECT ALL" }
  - { id: FR-0164-03, desc: "EXCEPT y EXCEPT ALL" }
nf:
  - { id: NF-0164-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0164-01
    given: dos SELECT unidos con UNION
    when: se ejecuta
    then: se devuelven filas unicas de ambos
    test: test_ac_0164_01_union
  - id: AC-0164-02
    given: UNION ALL
    when: se ejecuta
    then: se devuelven todas las filas (con duplicados)
    test: test_ac_0164_02_union_all
  - id: AC-0164-03
    given: INTERSECT
    when: se ejecuta
    then: se devuelven filas comunes
    test: test_ac_0164_03_intersect
  - id: AC-0164-04
    given: EXCEPT
    when: se ejecuta
    then: se devuelven filas del primero no en el segundo
    test: test_ac_0164_04_except
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-query --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0164 — Operaciones de conjuntos (UNION/INTERSECT/EXCEPT)

## Contexto

Las operaciones de conjuntos son esenciales para consultas analiticas y
comparacion de resultados. RuscaDB no las soporta.

## Superficie

- `ast.rs`: `Statement::SetOp { op, left, right, all }`.
- `parser.rs`: keywords `UNION`, `INTERSECT`, `EXCEPT`, `ALL`.
- `executor.rs`: materializar ambos lados y aplicar dedup/interseccion/diferencia.

## Estado del arte

- **SQL-92**: UNION/INTERSECT/EXCEPT con ALL opcional.
- **PostgreSQL/DuckDB**: soporte completo.
- **RuscaDB actual**: sin set operations.

## Diseño propuesto

1. Ejecutar ambos lados a RowSet.
2. UNION: concatenar + dedup (hash set por fila serializada).
3. UNION ALL: concatenar.
4. INTERSECT: filas presentes en ambos.
5. EXCEPT: filas del izquierdo no en el derecho.

## Riesgos

- Comparacion de filas con floats: usar serializacion canonica (Display).

## ADR requerido

`docs/adr/ADR-0164-rql-set-ops.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\rql_set_operations.md