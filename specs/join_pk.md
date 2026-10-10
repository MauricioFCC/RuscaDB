---
id: SPEC-0052
feature: join_pk
status: implemented
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb-query, ruscadb]
  out_of_scope: [LEFT/RIGHT/FULL JOIN, non-equi, self-join con alias, JOIN con KNN]
fr:
  - { id: FR-0052-01, desc: "INNER JOIN de dos tablas con ON de igualdad simple (a.x = b.y)" }
  - { id: FR-0052-02, desc: "ejecucion por nested-loop con lookups al indice primario (PK)" }
  - { id: FR-0052-03, desc: "columnas de salida cualificadas o fusionadas con prefijo de tabla" }
  - { id: FR-0052-04, desc: "sin coincidencias => 0 filas; tabla vacia => 0 filas" }
nf:
  - { id: NF-0052-01, desc: "SELECT sin JOIN sin cambios (sin regresion)" }
  - { id: NF-0052-02, desc: "ON no-equi o con mas de una condicion => error accionable" }
acceptance_criteria:
  - id: AC-0052-01
    given: dos tablas con claves coincidentes y una fila huerfana
    when: se ejecuta SELECT con INNER JOIN ... ON a.id = b.a_id
    then: devuelve solo las filas emparejadas con columnas de ambas tablas
    test: test_ac_0052_01_inner_join_matches
  - id: AC-0052-02
    given: dos tablas sin coincidencias
    when: se ejecuta el JOIN
    then: devuelve 0 filas
    test: test_ac_0052_02_no_match_empty
  - id: AC-0052-03
    given: una tabla vacia
    when: se ejecuta el JOIN
    then: devuelve 0 filas sin panics
    test: test_ac_0052_03_empty_table
  - id: AC-0052-04
    given: un ON con condicion no-equi
    when: se parsea o ejecuta
    then: devuelve error accionable (no soportado)
    test: test_ac_0052_04_non_equi_errors
exit_criteria:
  - cargo test -p ruscadb-query -- test_ac_0052
  - cargo test -p ruscadb -- test_ac_0052
  - cargo mutants -p ruscadb --in-place -f join.rs (MS >= 70% acotado)
rollback:
  - revertir query + fachada
sandbox:
  - cargo test -p ruscadb-query -p ruscadb
---

# SPEC-0052 — INNER JOIN de dos tablas por PK

## Contexto

Mayor hueco F2: `SELECT ... FROM a JOIN b ON a.x = b.y` (solo INNER, una sola
igualdad). Ejecución nested-loop con lookups al índice primario. Lógica del
executor en un módulo NUEVO `crates/ruscadb/src/join.rs` para no colisionar con
SPEC-0051. `ON` no-equi o múltiple => error accionable.

## Criterios de aceptación

- **AC-0052-01/02/03** — emparejados, sin coincidencias, tabla vacía.
- **AC-0052-04** — no-equi rechazado con error.

## Trazabilidad

Tests `test_ac_0052_<nn>_*`; verificado por `cargo xtask trace`.
