---
id: SPEC-0103
feature: outer_cross_join
status: implemented
owner: query-team
appetite_days: 3
boundaries:
  crates: [ruscadb-query, ruscadb]
  files: [crates/ruscadb-query/src/lexer.rs, crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/join.rs]
  out_of_scope: [JOIN con WHERE/ORDER BY/agregados (siguen fuera de alcance), no-equi, multi-condicion, mas de dos tablas]
fr:
  - { id: FR-0103-01, desc: "RQL acepta RIGHT [OUTER] JOIN, FULL [OUTER] JOIN y CROSS JOIN, con canonizacion Display y roundtrip; JoinClause.left/right pasan a Option (CROSS no tiene ON)" }
  - { id: FR-0103-02, desc: "el executor conserva las filas interiores sin contraparte en RIGHT (exteriores a NULL), las de ambos lados en FULL y el producto cartesiano en CROSS" }
nf:
  - { id: NF-0103-01, desc: "INNER y LEFT mantienen su semantica y su plan (lookup por indice o barrido); RIGHT/FULL reutilizan la misma ruta y solo anaden las filas sin contraparte" }
  - { id: NF-0103-02, desc: "el emparejamiento sigue siendo por una igualdad y NULL nunca empareja; CROSS no exige ON" }
acceptance_criteria:
  - id: AC-0103-01
    given: autores (uno sin libros) y libros (uno huerfano)
    when: se ejecuta RIGHT JOIN
    then: el libro huerfano aparece con las columnas del autor a NULL y no hay libros fantasma
    test: test_ac_0103_01_right_join_keeps_unmatched_inner
  - id: AC-0103-02
    given: el mismo dataset
    when: se ejecuta FULL JOIN
    then: aparecen el autor sin libros (books.* NULL) y el libro huerfano (authors.* NULL)
    test: test_ac_0103_02_full_join_keeps_both_sides
  - id: AC-0103-03
    given: 3 autores y 4 libros visibles
    when: se ejecuta CROSS JOIN
    then: devuelve 12 filas sin columnas NULL
    test: test_ac_0103_03_cross_join_is_cartesian
  - id: AC-0103-04
    given: texto RQL con RIGHT/FULL/CROSS JOIN
    when: se parsea y re-serializa
    then: canoniza (`RIGHT JOIN`/`FULL JOIN`/`CROSS JOIN`) y hace roundtrip; RIGHT sin JOIN es ParseError
    test: test_ac_0103_04_outer_and_cross_join_parse_and_display
exit_criteria:
  - cargo test -p ruscadb --test join_outer
  - cargo test -p ruscadb-query
  - cargo xtask trace / contract verdes
rollback:
  - retirar JoinKind Right/Full/Cross, el parseo y las ramas del executor
sandbox:
  - cargo test -p ruscadb --test join_outer
---

# SPEC-0103 — `RIGHT`/`FULL`/`CROSS JOIN` en RQL

## Contexto

Límite del MVP: «`JOIN` solo INNER de dos tablas (sin LEFT/RIGHT/FULL)». Tras
SPEC-0099 (LEFT), esta spec cierra el resto del álgebra de joins de dos tablas:
`RIGHT [OUTER]`, `FULL [OUTER]` y `CROSS JOIN`, sobre la misma ruta
nested-loop (lookup por índice si la columna interior está indexada).

## Superficie

- `ruscadb-query`: `JoinKind::{Right, Full, Cross}`, `JoinClause.left/right`
  como `Option<ColumnRef>` (CROSS sin `ON`) y `Display` canónico.
- `ruscadb`: ramas RIGHT/FULL (fila interior sin contraparte a `NULL`) y
  `cross_join` (producto cartesiano) en `execute_join_at`.

## Criterios de aceptación

- **AC-0103-01** — RIGHT conserva el lado interior.
- **AC-0103-02** — FULL conserva ambos lados.
- **AC-0103-03** — CROSS es cartesiano.
- **AC-0103-04** — parseo/canonización y error ruidoso.

## Trazabilidad

Tests `test_ac_0103_<nn>_*` con marcador `@spec AC-0103-NN`; verificado por
`cargo xtask trace`/`contract`.
