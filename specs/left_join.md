---
id: SPEC-0099
feature: left_join
status: implemented
owner: query-team
appetite_days: 3
boundaries:
  crates: [ruscadb-query, ruscadb]
  files: [crates/ruscadb-query/src/lexer.rs, crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/join.rs]
  out_of_scope: [RIGHT/FULL/CROSS JOIN, no-equi, multi-condicion, subconsultas, WHERE/ORDER BY con JOIN (siguen fuera de alcance)]
fr:
  - { id: FR-0099-01, desc: "RQL acepta LEFT [OUTER] JOIN ademas de INNER JOIN, con canonizacion Display y roundtrip" }
  - { id: FR-0099-02, desc: "el executor emite la fila exterior con las columnas interiores a NULL cuando no hay emparejamiento (LEFT), conservando el orden exterior" }
nf:
  - { id: NF-0099-01, desc: "INNER JOIN mantiene comportamiento y plan (lookup por indice o barrido); LEFT reutiliza la misma ruta y solo anade el caso sin match" }
  - { id: NF-0099-02, desc: "NULL exterior nunca empareja (como en INNER); las columnas interiores sin match se proyectan como NULL" }
acceptance_criteria:
  - id: AC-0099-01
    given: una tabla exterior con una fila sin contraparte y otra con contrapartes
    when: se ejecuta LEFT JOIN
    then: la fila sin contraparte aparece una vez con las columnas interiores a NULL y las demas con sus valores
    test: test_ac_0099_01_left_join_keeps_unmatched_outer
  - id: AC-0099-02
    given: el mismo dataset
    when: se compara INNER JOIN con LEFT JOIN
    then: INNER omite la fila sin contraparte y LEFT la incluye (diferencia exacta de una fila)
    test: test_ac_0099_02_inner_vs_left_difference
  - id: AC-0099-03
    given: texto RQL con LEFT JOIN, LEFT OUTER JOIN y JOIN
    when: se parsea y re-serializa
    then: canoniza LEFT JOIN / JOIN y hace roundtrip
    test: test_ac_0099_03_left_join_parses_and_displays
exit_criteria:
  - cargo test -p ruscadb --test join_left
  - cargo test -p ruscadb-query
  - cargo xtask trace / contract verdes
rollback:
  - retirar JoinKind y la rama LEFT del executor (volver a INNER-only)
sandbox:
  - cargo test -p ruscadb --test join_left
---

# SPEC-0099 — `LEFT JOIN` en RQL

## Contexto

Límite del MVP: «`JOIN` solo INNER de dos tablas con una igualdad (sin
LEFT/RIGHT/FULL)». Se implementa `LEFT [OUTER] JOIN` sobre la misma ruta
nested-loop (lookup por índice si la columna interior está indexada; barrido
filtrado si no), emitiendo la fila exterior con las columnas interiores a
`NULL` cuando no hay emparejamiento.

## Superficie

- `ruscadb-query`: `Keyword::Left`/`Outer`, `JoinKind{Inner, Left}`,
  `JoinClause::kind`, parseo `LEFT [OUTER] JOIN` y `Display` canónico.
- `ruscadb`: rama LEFT en `execute_join_at` (`merge_outer_only`).

## Criterios de aceptación

- **AC-0099-01** — fila exterior sin contraparte con interiores `NULL`.
- **AC-0099-02** — diferencia exacta INNER vs LEFT.
- **AC-0099-03** — parseo/canonización/roundtrip.

## Trazabilidad

Tests `test_ac_0099_<nn>_*` con marcador `@spec AC-0099-NN`; verificado por
`cargo xtask trace`/`contract`.
