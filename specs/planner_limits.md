---
id: SPEC-0054
feature: planner_limits
status: accepted
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb-query, ruscadb]
  out_of_scope: [optimizador de joins, estadísticas de tablas, paralelismo]
fr:
  - { id: FR-0054-01, desc: "budget de complejidad 100000 unidades; el planner rechaza la query antes de ejecutar" }
  - { id: FR-0054-02, desc: "profundidad AST maxima 64 (guard clause, sin recursion descontrolada)" }
  - { id: FR-0054-03, desc: "LIMIT implicito 1000 inyectado por el planner si falta" }
  - { id: FR-0054-04, desc: "timeout cooperativo 5000 ms con cancel token" }
  - { id: FR-0054-05, desc: "EXPLAIN advierte full-scan sobre umbral (R9)" }
nf:
  - { id: NF-0054-01, desc: "queries normales sin cambios de resultado (cero regresion)" }
  - { id: NF-0054-02, desc: "errores accionables WHAT+WHY+WHERE, sin panics" }
acceptance_criteria:
  - id: AC-0054-01
    given: una query con anidamiento > 64 niveles
    when: se parsea
    then: ParseError accionable mencionando el limite
    test: test_ac_0054_01_deep_nesting_rejected
  - id: AC-0054-02
    given: un WHERE con cadena OR que supera el budget
    when: se planifica
    then: rechazo pre-ejecucion mencionando el budget
    test: test_ac_0054_02_or_chain_over_budget_rejected
  - id: AC-0054-03
    given: un SELECT sin LIMIT sobre tabla con > 1000 filas
    when: se ejecuta
    then: devuelve como maximo 1000 filas
    test: test_ac_0054_03_implicit_limit_injected
  - id: AC-0054-04
    given: un scan con deadline expirado
    when: se ejecuta
    then: error de timeout, sin colgar ni panic
    test: test_ac_0054_04_timeout_cancels_scan
  - id: AC-0054-05
    given: un SELECT con full scan inevitable
    when: se pide EXPLAIN
    then: el plan incluye aviso de full-scan
    test: test_ac_0054_05_explain_flags_full_scan
exit_criteria:
  - cargo test -p ruscadb-query -p ruscadb -- test_ac_0054
  - suite previa de query (54 tests) verde
  - cargo mutants acotado a los ficheros tocados MS >= 70%
rollback:
  - revertir query + fachada (guard clauses detras de consts)
sandbox:
  - cargo test -p ruscadb-query -p ruscadb
---

# SPEC-0054 — Límites de recursos del planner (§6.4, R9)

## Contexto

Release-blocking de seguridad: el parser/executor son superficie DoS
(STRIDE §6.2). Coste estático pre-ejecución (query-cost analysis), deadline
cooperativo y `EXPLAIN` como gate de observabilidad. Constantes con nombre,
sin magic numbers.

## Criterios de aceptación

- **AC-0054-01/02** — rechazo estático (profundidad + budget).
- **AC-0054-03/04/05** — LIMIT implícito, timeout cooperativo, aviso EXPLAIN.

## Trazabilidad

Tests `test_ac_0054_<nn>_*`; verificado por `cargo xtask trace`.
