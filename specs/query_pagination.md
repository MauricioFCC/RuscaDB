---
id: SPEC-0096
feature: query_pagination
status: implemented
owner: query-team
appetite_days: 2
boundaries:
  crates: [ruscadb-query, ruscadb]
  files: [crates/ruscadb-query/src/lexer.rs, crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/executor.rs, crates/ruscadb/src/join.rs]
  out_of_scope: [cursores con estado/servidor, streaming, keyset pagination, LIMIT/OFFSET en DML]
fr:
  - { id: FR-0096-01, desc: "RQL acepta LIMIT n OFFSET m (y OFFSET n sin LIMIT, semantica Postgres) con canonizacion Display y roundtrip" }
  - { id: FR-0096-02, desc: "OFFSET descarta las primeras m filas tras ORDER BY en la ruta de escaneo, la de agregacion y el JOIN" }
nf:
  - { id: NF-0096-01, desc: "el LIMIT implicito anti-DoS (planner) se aplica despues del OFFSET; OFFSET > filas devuelve vacio sin error" }
  - { id: NF-0096-02, desc: "OFFSET mal formado (sin numero o no entero) es ParseError accionable; sin panics" }
acceptance_criteria:
  - id: AC-0096-01
    given: una tabla con 5 filas ordenables
    when: se consulta LIMIT/OFFSET en distintos puntos de la ventana (0, intermedio, == filas, > filas, LIMIT 0)
    then: devuelve exactamente la ventana pedida y vacio en los bordes fuera de rango
    test: test_ac_0096_01_limit_offset_window
  - id: AC-0096-02
    given: una tabla con dos grupos
    when: se consulta GROUP BY con ORDER BY LIMIT 1 OFFSET 1
    then: se pagina el segundo grupo con su agregado correcto
    test: test_ac_0096_02_offset_applies_to_group_by
  - id: AC-0096-03
    given: texto RQL con OFFSET bien y mal formado
    when: se parsea y se re-serializa
    then: el valido canoniza (LIMIT n OFFSET m) y hace roundtrip; el mal formado es ParseError
    test: test_ac_0096_03_offset_parses_and_rejects_malformed
exit_criteria:
  - cargo test -p ruscadb --test query_pagination
  - cargo test -p ruscadb-query
  - cargo xtask trace / contract verdes
rollback:
  - retirar la palabra clave OFFSET, el campo offset y su aplicacion en executor/join
sandbox:
  - cargo test -p ruscadb --test query_pagination
---

# SPEC-0096 — Paginación `LIMIT ... OFFSET` en RQL

## Contexto

Límite del MVP: «sin streaming ni paginación por cursor (`execute` materializa
las filas)». Se cierra la parte de **paginación por offset** (la más usada en
APIs) sin introducir cursores con estado: `OFFSET` descarta las primeras `m`
filas del resultado ya ordenado.

Frontera 2026 (Postgres/DuckDB): `OFFSET n` sin `LIMIT` es válido; el `OFFSET`
se aplica **después** de `ORDER BY` y antes del `LIMIT`. El `LIMIT` implícito
anti-DoS del planner (SPEC-0054) sigue vigente y se aplica tras el `OFFSET`.

## Superficie

- `ruscadb-query`: `Keyword::Offset`, campo `Select::offset`, parseo tras
  `LIMIT` y `Display` canónico.
- `ruscadb`: aplicación del `OFFSET` en la ruta de escaneo, la de agregación
  (`GROUP BY`) y el `JOIN`.

## Criterios de aceptación

- **AC-0096-01** — ventana correcta y bordes (0, ==, >, `LIMIT 0`).
- **AC-0096-02** — `OFFSET` en la ruta de agregación.
- **AC-0096-03** — parseo/canonización y errores ruidosos.

## Trazabilidad

Tests `test_ac_0096_<nn>_*` con marcador `@spec AC-0096-NN`; verificado por
`cargo xtask trace`/`contract`.
