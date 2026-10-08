---
id: SPEC-0036
feature: order_by
status: accepted
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb-query, ruscadb]
  out_of_scope: [ORDER BY por expresion, NULLS FIRST/LAST configurable, multi-clave]
fr:
  - { id: FR-0036-01, desc: "parsear ORDER BY <col> [ASC|DESC] como parte del Select" }
  - { id: FR-0036-02, desc: "Display y roundtrip de la clausula" }
  - { id: FR-0036-03, desc: "el executor ordena las filas antes de LIMIT" }
  - { id: FR-0036-04, desc: "NULL ordena al final en ASC (y al principio en DESC); tipos incompatibles => error" }
nf:
  - { id: NF-0036-01, desc: "orden O(n log n) estable; sin regresion en queries sin ORDER BY" }
  - { id: NF-0036-02, desc: "sin panics; columna inexistente => ColumnNotFound" }
acceptance_criteria:
  - id: AC-0036-01
    given: "SELECT * FROM t ORDER BY a"
    when: se parsea
    then: el Select lleva order_by Some{column:"a", desc:false}
    test: test_ac_0036_01_parse_order_by
  - id: AC-0036-02
    given: una tabla con valores desordenados
    when: se ejecuta "SELECT * FROM t ORDER BY a ASC"
    then: las filas salen ordenadas ascendente
    test: test_ac_0036_02_execute_order_asc
  - id: AC-0036-03
    given: la misma tabla
    when: se ejecuta "SELECT * FROM t ORDER BY a DESC LIMIT 2"
    then: salen las 2 mayores en orden descendente
    test: test_ac_0036_03_execute_order_desc_limit
  - id: AC-0036-04
    given: una tabla con NULLs
    when: se ordena ASC
    then: los NULL van al final
    test: test_ac_0036_04_nulls_last_asc
  - id: AC-0036-05
    given: una query con ORDER BY sobre columna inexistente
    when: se ejecuta
    then: ColumnNotFound accionable
    test: test_ac_0036_05_order_by_unknown_column
exit_criteria:
  - cargo test -p ruscadb-query -p ruscadb -- test_ac_0036
  - cargo mutants -p ruscadb-query mutation score >= 70%
rollback:
  - revertir query + fachada
sandbox:
  - cargo test -p ruscadb-query -p ruscadb
---

# SPEC-0036 — `ORDER BY` (lenguaje + executor)

## Contexto

Feature SQL básica ausente. Se extiende el IR/parser (`ruscadb-query`) con
`order_by: Option<OrderBy { column, desc }>` y el executor ordena las filas
**antes** de `LIMIT`. `Display` canónico `... ORDER BY col [DESC]` (roundtrip).
NULL al final en ASC. Tipos incompatibles → `TypeMismatch`; columna inexistente
→ `ColumnNotFound`.

## Criterios de aceptación

- **AC-0036-01** — parseo/roundtrip.
- **AC-0036-02/03** — orden ASC/DESC + LIMIT.
- **AC-0036-04/05** — NULLs y columna inexistente.

## Trazabilidad

Tests `test_ac_0036_<nn>_*`; verificado por `cargo xtask trace`.
