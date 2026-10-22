---
id: SPEC-0108
feature: join_where_order
status: implemented
owner: query-team
appetite_days: 3
boundaries:
  crates: [ruscadb, ruscadb-query]
  files: [crates/ruscadb/src/join.rs, crates/ruscadb/tests/join_where_order.rs]
  out_of_scope: [agregados con JOIN, GROUP BY/HAVING con JOIN, KNN/TRAVERSE con JOIN, ORDER BY por expresion]
fr:
  - { id: FR-0108-01, desc: "el JOIN admite WHERE sobre columnas cualificadas o no cualificadas unicas" }
  - { id: FR-0108-02, desc: "el JOIN admite ORDER BY col [ASC|DESC] sobre filas fusionadas" }
nf:
  - { id: NF-0108-01, desc: "WHERE y ORDER BY se aplican tras fusionar y antes de LIMIT/OFFSET" }
acceptance_criteria:
  - id: AC-0108-01
    given: un INNER JOIN con WHERE sobre columna fusionada
    when: se ejecuta
    then: solo las filas que cumplen el filtro se proyectan
    test: test_ac_0108_01_join_where_filters_rows
  - id: AC-0108-02
    given: un JOIN con ORDER BY DESC
    when: se ejecuta
    then: las filas se devuelven ordenadas
    test: test_ac_0108_02_join_order_by_desc
exit_criteria:
  - cargo test -p ruscadb --all-features --test join_where_order
  - cargo test --workspace --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0108 — JOIN con WHERE/ORDER BY

## Contexto

RuscaDB soporta JOIN (INNER/LEFT/RIGHT/FULL/CROSS) pero rechaza `WHERE` y
`ORDER BY` (SPEC-0052/SPEC-0099). Esta spec habilita filtrado y ordenamiento
sobre las filas fusionadas, aplicados tras la fusion y antes de LIMIT/OFFSET.

## Superficie

- `crates/ruscadb/src/join.rs` — `reject_unsupported` deja pasar `filter` y
  `order_by`; `apply_join_filter_and_sort` filtra/ordena las filas fusionadas.
- Columnas cualificadas (`tabla.columna`) o no cualificadas si son unicas.
---
