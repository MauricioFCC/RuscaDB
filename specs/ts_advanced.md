---
id: SPEC-0039
feature: ts_advanced
status: accepted
owner: data-team
appetite_days: 4
boundaries:
  crates: [ruscadb-ts]
  out_of_scope: [integracion en RQL, series en disco]
fr:
  - { id: FR-0039-01, desc: "percentile(points, p): percentil con interpolacion lineal (p en [0,100])" }
  - { id: FR-0039-02, desc: "rate(points): tasa por segundo entre puntos consecutivos ((v2-v1)/(t2-t1))" }
  - { id: FR-0039-03, desc: "moving_average(points, window): media movil simple por ventana de N puntos" }
  - { id: FR-0039-04, desc: "errores accionables ante p fuera de rango o ventana <= 0" }
nf:
  - { id: NF-0039-01, desc: "determinista; sin panics; O(n log n) por ordenacion" }
  - { id: NF-0039-02, desc: "percentile con un solo punto = ese valor; rate con <2 puntos = vacio" }
acceptance_criteria:
  - id: AC-0039-01
    given: valores conocidos
    when: se calcula el percentil 50 (mediana)
    then: coincide con la interpolacion lineal esperada
    test: test_ac_0039_01_percentile_interpolates
  - id: AC-0039-02
    given: una serie temporal
    when: se calcula rate
    then: devuelve las tasas por segundo entre puntos consecutivos
    test: test_ac_0039_02_rate_per_second
  - id: AC-0039-03
    given: una serie
    when: se calcula la media movil con ventana N
    then: el resultado tiene la longitud esperada y los valores correctos
    test: test_ac_0039_03_moving_average
  - id: AC-0039-04
    given: p fuera de [0,100] o ventana <= 0
    when: se opera
    then: error accionable (sin panics)
    test: test_ac_0039_04_invalid_inputs
  - id: AC-0039-05
    given: series vacias o de 1 punto
    when: se opera
    then: percentile = el valor; rate y moving_average = vacio (sin panics)
    test: test_ac_0039_05_boundary_series
exit_criteria:
  - cargo test -p ruscadb-ts -- test_ac_0039
  - cargo mutants -p ruscadb-ts mutation score >= 70%
rollback:
  - revertir russcadb-ts
sandbox:
  - cargo test -p ruscadb-ts
---

# SPEC-0039 — Agregados avanzados de serie temporal

## Contexto

Extiende `ruscadb-ts` (SPEC-0029) con operaciones analíticas comunes:
`percentile`, `rate`, `moving_average`. Deterministas, sobre `SeriesPoint`.

## Criterios de aceptación

- **AC-0039-01..03** — percentil, rate, media móvil.
- **AC-0039-04/05** — entradas inválidas y fronteras.

## Trazabilidad

Tests `test_ac_0039_<nn>_*` + proptests; verificado por `cargo xtask trace`.
