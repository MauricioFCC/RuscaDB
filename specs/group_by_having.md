---
id: SPEC-0051
feature: group_by_having
status: implemented
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb-query, ruscadb]
  out_of_scope: [DISTINCT, HAVING sin GROUP BY, agregados anidados]
fr:
  - { id: FR-0051-01, desc: "GROUP BY con multiples claves separadas por comas" }
  - { id: FR-0051-02, desc: "HAVING con comparaciones sobre agregados (COUNT/SUM/AVG/MIN/MAX) y AND" }
  - { id: FR-0051-03, desc: "HAVING sin GROUP BY => error de parseo" }
  - { id: FR-0051-04, desc: "grupos vacios (tabla vacia) => 0 filas; el agregado sobre grupo filtra por HAVING" }
nf:
  - { id: NF-0051-01, desc: "GROUP BY de una clave sigue funcionando igual (sin regresion)" }
  - { id: NF-0051-02, desc: "sin panics; claves inexistentes => error accionable" }
acceptance_criteria:
  - id: AC-0051-01
    given: filas con dos columnas de agrupacion
    when: se ejecuta GROUP BY a, b con COUNT
    then: cada combinacion (a,b) forma un grupo con su conteo
    test: test_ac_0051_01_multi_key_groups
  - id: AC-0051-02
    given: grupos con conteos distintos
    when: se ejecuta GROUP BY a HAVING COUNT(*) > 1
    then: solo los grupos con mas de una fila sobreviven
    test: test_ac_0051_02_having_filters_groups
  - id: AC-0051-03
    given: una consulta con HAVING y sin GROUP BY
    when: se parsea
    then: devuelve error de parseo accionable
    test: test_ac_0051_03_having_without_group_by_errors
  - id: AC-0051-04
    given: una tabla vacia
    when: se ejecuta GROUP BY con HAVING
    then: devuelve 0 filas sin panics
    test: test_ac_0051_04_empty_table
  - id: AC-0051-05
    given: GROUP BY de una sola clave (comportamiento previo)
    when: se ejecuta
    then: sin regresion
    test: test_ac_0051_05_single_key_unchanged
exit_criteria:
  - cargo test -p ruscadb-query -- test_ac_0051
  - cargo test -p ruscadb -- test_ac_0051
  - cargo mutants -p ruscadb-query --in-place -f parser.rs,ast.rs (MS >= 70% acotado)
rollback:
  - revertir query + fachada
sandbox:
  - cargo test -p ruscadb-query -p ruscadb
---

# SPEC-0051 — GROUP BY multi-clave + HAVING

## Contexto

Cierra F2: `GROUP BY a, b` (hash por tupla de claves) y `HAVING <cmp sobre
agregado> [AND ...]`. `HAVING` sin `GROUP BY` es error de parseo. La agregación
de una clave (SPEC-0041) no cambia.

## Criterios de aceptación

- **AC-0051-01/02** — grupos multi-clave y filtrado por HAVING.
- **AC-0051-03/04/05** — error sin GROUP BY, tabla vacía y no regresión.

## Trazabilidad

Tests `test_ac_0051_<nn>_*`; verificado por `cargo xtask trace`.
