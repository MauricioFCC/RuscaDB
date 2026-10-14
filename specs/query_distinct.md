---
id: SPEC-0097
feature: query_distinct
status: implemented
owner: query-team
appetite_days: 2
boundaries:
  crates: [ruscadb-query, ruscadb]
  files: [crates/ruscadb-query/src/lexer.rs, crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/executor.rs, crates/ruscadb/src/join.rs]
  out_of_scope: [DISTINCT ON/agregados DISTINCT internos, dedupe por subconjunto de columnas, hashing de filas]
fr:
  - { id: FR-0097-01, desc: "RQL acepta SELECT DISTINCT (proyeccion completa) con canonizacion Display y roundtrip" }
  - { id: FR-0097-02, desc: "el executor deduplica las filas proyectadas preservando el orden de primera aparicion, en escaneo, agregacion y JOIN, antes de OFFSET/LIMIT" }
nf:
  - { id: NF-0097-01, desc: "dedupe O(n^2) sobre filas proyectadas acotadas por el LIMIT implicito anti-DoS; sin dependencias nuevas" }
  - { id: NF-0097-02, desc: "DISTINCT no altera el orden relativo de la primera aparicion ni el resto de clausulas" }
acceptance_criteria:
  - id: AC-0097-01
    given: una tabla con filas duplicadas en la proyeccion
    when: se consulta SELECT DISTINCT (con y sin ORDER BY)
    then: devuelve una fila por combinacion distinta, preservando la primera aparicion
    test: test_ac_0097_01_distinct_dedups_projection
  - id: AC-0097-02
    given: filas con duplicados y un LIMIT/OFFSET
    when: se combina SELECT DISTINCT con ORDER BY y LIMIT/OFFSET
    then: primero deduplica y luego pagina sobre el resultado unico
    test: test_ac_0097_02_distinct_before_pagination
  - id: AC-0097-03
    given: texto RQL con DISTINCT bien formado
    when: se parsea y re-serializa
    then: canoniza `SELECT DISTINCT` y hace roundtrip
    test: test_ac_0097_03_distinct_parses_and_displays
exit_criteria:
  - cargo test -p ruscadb --test query_distinct
  - cargo test -p ruscadb-query
  - cargo xtask trace / contract verdes
rollback:
  - retirar la palabra clave DISTINCT, el campo distinct y el dedupe
sandbox:
  - cargo test -p ruscadb --test query_distinct
---

# SPEC-0097 — `SELECT DISTINCT` en RQL

## Contexto

Límite del MVP: RQL «sin `DISTINCT`». Se implementa la forma estándar
(proyección completa) con deduplicación de filas proyectadas preservando el
orden de primera aparición, aplicada **antes** de `OFFSET`/`LIMIT`
(SPEC-0096) para que la paginación cuente filas únicas.

## Superficie

- `ruscadb-query`: `Keyword::Distinct`, campo `Select::distinct`, parseo tras
  `SELECT` y `Display` canónico.
- `ruscadb`: `dedup_rows` aplicado en las rutas de escaneo, agregación y `JOIN`.

## Criterios de aceptación

- **AC-0097-01** — dedupe de la proyección preservando orden.
- **AC-0097-02** — `DISTINCT` antes de `LIMIT`/`OFFSET`.
- **AC-0097-03** — parseo/canonización/roundtrip.

## Trazabilidad

Tests `test_ac_0097_<nn>_*` con marcador `@spec AC-0097-NN`; verificado por
`cargo xtask trace`/`contract`.
