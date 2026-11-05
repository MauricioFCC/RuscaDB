---
id: SPEC-0204
feature: explain_analyze
status: implemented
maturity: real
owner: query-team
appetite_days: 1
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/table_api.rs
    - crates/ruscadb/tests/explain_analyze.rs
  out_of_scope: [per-operator timings, buffers/IO stats, profiling continuo]
fr:
  - { id: FR-0204-01, desc: "EXPLAIN ANALYZE ejecuta el SELECT y reporta filas y tiempo reales" }
  - { id: FR-0204-02, desc: "solo admite EXPLAIN SELECT; DML y sentencias sin EXPLAIN fallan" }
nf:
  - { id: NF-0204-01, desc: "una sola ejecución; wall-clock local, no se usa como gate" }
  - { id: NF-0204-02, desc: "mismas lineas que EXPLAIN mas ACTUAL_ROWS/ACTUAL_TIME_MS" }
acceptance_criteria:
  - id: AC-0204-01
    given: una tabla pequena y un EXPLAIN SELECT
    when: se ejecuta EXPLAIN ANALYZE
    then: incluye ACTUAL_ROWS correcto y ACTUAL_TIME_MS parseable
    test: test_ac_0204_01_explain_analyze_reports_actuals
  - id: AC-0204-02
    given: una sentencia sin EXPLAIN o un EXPLAIN de DML
    when: se ejecuta EXPLAIN ANALYZE
    then: devuelve error accionable
    test: test_ac_0204_02_explain_analyze_rejects_non_select
exit_criteria:
  - cargo test -p ruscadb --test explain_analyze
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar el metodo; EXPLAIN sigue igual
sandbox:
  - cargo test -p ruscadb --test explain_analyze
---
# SPEC-0204 — EXPLAIN ANALYZE

## Contexto

`EXPLAIN` muestra el plan y estimaciones, pero no filas/tiempo reales.
PostgreSQL `EXPLAIN ANALYZE` es la herramienta estándar para cerrar el bucle
planner↔ejecución.

## Diseño

1. `Database::explain_analyze(query)` valida con `explain` (solo SELECT).
2. Ejecuta el SELECT interno una vez y mide `Instant`.
3. Añade `ACTUAL_ROWS` y `ACTUAL_TIME_MS` al plan.
4. El tiempo es informativo/local, no gate.

## Complejidad

```text
coste = coste(EXPLAIN) + coste(SELECT) + O(1)
memoria = O(lineas del plan)
```

## TDD adversarial

Filas reales vs `execute`; rechazo de sentencias sin EXPLAIN y de DML.
