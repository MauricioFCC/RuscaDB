---
id: SPEC-0040
feature: group_by_aggregates
status: accepted
owner: query-team
appetite_days: 8
boundaries:
  crates: [ruscadb-query, ruscadb]
  out_of_scope: [HAVING, GROUP BY por expresion, DISTINCT, agregados sobre KNN/TRAVERSE]
fr:
  - { id: FR-0040-01, desc: "parsear agregados COUNT(*)/COUNT(col)/SUM/AVG/MIN/MAX en la proyeccion" }
  - { id: FR-0040-02, desc: "parsear GROUP BY <col>[, <col>...]" }
  - { id: FR-0040-03, desc: "el executor agrupa (hash aggregation) y calcula los agregados" }
  - { id: FR-0040-04, desc: "COUNT(*) cuenta filas; COUNT(col) ignora NULL; SUM/AVG/MIN/MAX ignoran NULL" }
  - { id: FR-0040-05, desc: "sin GROUP BY y con agregados => una sola fila agregada" }
nf:
  - { id: NF-0040-01, desc: "agrupacion O(n) esperada (HashMap/BTreeMap); sin regresion en queries sin agregados" }
  - { id: NF-0040-02, desc: "errores accionables: columna no agrupada en la proyeccion, tipo no agregable, GROUP BY inexistente" }
acceptance_criteria:
  - id: AC-0040-01
    given: "SELECT a, COUNT(*) FROM t GROUP BY a"
    when: se parsea
    then: el Select lleva group_by [a] y una proyeccion con COUNT(*)
    test: test_ac_0040_01_parse_group_by
  - id: AC-0040-02
    given: filas con la misma clave
    when: se ejecuta COUNT(*) agrupado
    then: cada grupo tiene el conteo correcto
    test: test_ac_0040_02_count_groups
  - id: AC-0040-03
    given: valores numericos con NULLs
    when: se ejecuta SUM/AVG
    then: los NULL se ignoran y el resultado es correcto
    test: test_ac_0040_03_sum_avg_ignore_nulls
  - id: AC-0040-04
    given: valores variados
    when: se ejecuta MIN/MAX
    then: devuelven el minimo y maximo por grupo
    test: test_ac_0040_04_min_max
  - id: AC-0040-05
    given: agregados sin GROUP BY
    when: se ejecuta
    then: devuelve una sola fila con el agregado global
    test: test_ac_0040_05_aggregate_without_group_by
exit_criteria:
  - cargo test -p ruscadb-query -p ruscadb -- test_ac_0040
  - cargo mutants -p ruscadb-query mutation score >= 70%
rollback:
  - revertir query + fachada
sandbox:
  - cargo test -p ruscadb-query -p ruscadb
---

# SPEC-0040 — `GROUP BY` + agregados (COUNT/SUM/AVG/MIN/MAX)

## Contexto

Feature analítica central (patrón *hash aggregation*, como DuckDB/DataFusion).
Se extiende el IR (`AggFunc`, `Expr::Aggregate`, `Select.group_by`) y el
executor agrupa por las columnas indicadas y calcula los agregados, respetando
la semántica SQL de NULL (COUNT(*) cuenta filas; el resto ignora NULL).
`Display` canónico y roundtrip.

## Criterios de aceptación

- **AC-0040-01** — parseo/roundtrip.
- **AC-0040-02/03/04** — COUNT/SUM/AVG/MIN/MAX con NULLs.
- **AC-0040-05** — agregado global sin GROUP BY.

## Trazabilidad

Tests `test_ac_0040_<nn>_*`; verificado por `cargo xtask trace`.
