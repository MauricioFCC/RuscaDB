---
id: SPEC-0057
feature: ts_facade
status: accepted
owner: facade-team
appetite_days: 5
boundaries:
  crates: [ruscadb-ts, ruscadb]
  out_of_scope: [particion temporal en disco, downsampling continuo, joins temporales]
fr:
  - { id: FR-0057-01, desc: "ruscadb-ts como dependencia de la fachada (nuevo modulo ts_api)" }
  - { id: FR-0057-02, desc: "BUCKET(ts, …) sobre scalars['ts'] en RQL con agregados" }
  - { id: FR-0057-03, desc: "WINDOW deslizante con agregados basicos" }
  - { id: FR-0057-04, desc: "columna no temporal => error accionable; tabla vacia => 0 filas" }
nf:
  - { id: NF-0057-01, desc: "cero regresion en SELECT/GROUP BY existentes" }
  - { id: NF-0057-02, desc: "sin panics; buckets vacios se omiten (no NULL fantasma)" }
acceptance_criteria:
  - id: AC-0057-01
    given: filas con ts crecientes
    when: se ejecuta BUCKET por intervalo con COUNT
    then: cada bucket tiene su conteo exacto
    test: test_ac_0057_01_bucket_aggregation
  - id: AC-0057-02
    given: una serie con ventana deslizante
    when: se ejecuta WINDOW con SUM
    then: coincide con el oraculo ingenuo
    test: test_ac_0057_02_window_slides
  - id: AC-0057-03
    given: una tabla vacia
    when: se ejecuta BUCKET/WINDOW
    then: 0 filas sin panics
    test: test_ac_0057_03_empty_table
  - id: AC-0057-04
    given: BUCKET sobre columna no temporal
    when: se ejecuta
    then: error accionable (no soportado/no temporal)
    test: test_ac_0057_04_non_ts_column_errors
exit_criteria:
  - cargo test -p ruscadb-ts -p ruscadb -- test_ac_0057
  - PBT vs oraculo ingenuo en memoria
  - cargo mutants acotado MS >= 70%
rollback:
  - revertir ts + fachada (modulo nuevo, hook minimo)
sandbox:
  - cargo test -p ruscadb-ts -p ruscadb
---

# SPEC-0057 — Time-series cableado en la fachada

## Contexto

`ruscadb-ts` (bucket/window/resample/percentile/rate/moving_average) existe
pero la fachada no lo usa (`Cargo.toml` sin la dependencia). Se cablea con un
módulo NUEVO `ts_api.rs` + hook mínimo en la ruta SELECT, sin tocar el
agregador existente. Partición temporal en disco queda fuera (futuro).

## Criterios de aceptación

- **AC-0057-01/02** — buckets y ventanas vs oráculo.
- **AC-0057-03/04** — vacía y columna no temporal.

## Trazabilidad

Tests `test_ac_0057_<nn>_*`; verificado por `cargo xtask trace`.
