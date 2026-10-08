---
id: SPEC-0029
feature: time_series
status: accepted
owner: data-team
appetite_days: 5
boundaries:
  crates: [ruscadb-ts, ruscadb-core]
  out_of_scope: [persistencia de series, integracion en RQL, downsampling en disco]
fr:
  - { id: FR-0029-01, desc: "time_bucket(ts_ms, bucket_ms): piso al inicio del cubo (error si bucket<=0)" }
  - { id: FR-0029-02, desc: "ventanas tumbling y deslizantes con agregados count/sum/min/max/avg" }
  - { id: FR-0029-03, desc: "remuestreo con relleno (previous/null) sobre una rejilla uniforme" }
  - { id: FR-0029-04, desc: "SeriesPoint {ts_ms, value} ordenable y serializable" }
nf:
  - { id: NF-0029-01, desc: "bucketing O(1) por punto; ventanas O(n log n) por ordenacion" }
  - { id: NF-0029-02, desc: "sin panics; ts negativos y bucket no divisor se manejan (floor correcto)" }
acceptance_criteria:
  - id: AC-0029-01
    given: timestamps concretos y un bucket de 1h
    when: se aplica time_bucket
    then: cada ts cae en el inicio de su cubo
    test: test_ac_0029_01_time_bucket_floors
  - id: AC-0029-02
    given: puntos con valores
    when: se agregan en una ventana con count/sum/min/max/avg
    then: los agregados coinciden con el cálculo manual
    test: test_ac_0029_02_window_aggregates
  - id: AC-0029-03
    given: puntos dispersos y una rejilla uniforme
    when: se remuestrea con relleno previous
    then: los huecos se rellenan con el ultimo valor y el primero con None
    test: test_ac_0029_03_resample_fills
  - id: AC-0029-04
    given: puntos desordenados
    when: se ordenan y se hace una ventana deslizante
    then: el orden no altera el resultado (determinista)
    test: test_ac_0029_04_ordering_is_irrelevant
  - id: AC-0029-05
    given: bucket<=0 o ventana vacia
    when: se opera
    then: error accionable o resultado vacio (sin panics)
    test: test_ac_0029_05_invalid_inputs_are_safe
exit_criteria:
  - cargo test -p ruscadb-ts -- test_ac_0029
  - cargo mutants -p ruscadb-ts mutation score >= 70%
rollback:
  - eliminar el crate ruscadb-ts del workspace
sandbox:
  - cargo test -p ruscadb-ts
---

# SPEC-0029 — Serie temporal (bucketing, ventanas, remuestreo)

## Contexto

Completa el quinto modelo del roadmap (§5.2) sobre `ScalarValue::TimestampMillis`:
un crate puro `ruscadb-ts` con las operaciones temporales básicas, sin
persistencia ni sintaxis RQL (siguiente iteración):

- `time_bucket(ts_ms, bucket_ms) -> Result<i64, RuscaError>`.
- `window(points, window_ms, step_ms, agg) -> Vec<Window>` (tumbling si
  `step == window`; deslizante si `step < window`).
- `resample(points, start, end, step, fill) -> Vec<Option<f64>>`.

## Criterios de aceptación

- **AC-0029-01..04** — bucketing, agregados, remuestreo, orden.
- **AC-0029-05** — entradas inválidas seguras.

## Trazabilidad

Tests `test_ac_0029_<nn>_*` + proptests; verificado por `cargo xtask trace`.
