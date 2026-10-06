---
id: SPEC-0005
feature: query_language
status: accepted
owner: query-team
appetite_days: 8
boundaries:
  crates: [ruscadb-query, ruscadb-core]
  out_of_scope: [execution, planner, vector, graph]
fr:
  - { id: FR-0005-01, desc: "parsear SELECT con proyeccion (* o columnas) y FROM" }
  - { id: FR-0005-02, desc: "parsear WHERE con comparaciones y AND" }
  - { id: FR-0005-03, desc: "parsear LIMIT numerico" }
  - { id: FR-0005-04, desc: "reportar ParseError con posicion ante entrada invalida" }
  - { id: FR-0005-05, desc: "roundtrip Display->parse estable (metamorphic)" }
nf:
  - { id: NF-0005-01, desc: "parse O(n) sobre el texto de la consulta" }
  - { id: NF-0005-02, desc: "sin panics ante entrada arbitraria (fuzz/proptest)" }
acceptance_criteria:
  - id: AC-0005-01
    given: la consulta "SELECT a, b FROM t"
    when: se parsea
    then: produce un Select con proyeccion [a, b] y tabla t
    test: test_ac_0005_01_parse_simple_select
  - id: AC-0005-02
    given: la consulta "SELECT * FROM t WHERE a = 1 AND b < 2"
    when: se parsea
    then: produce un filtro con AND de dos comparaciones
    test: test_ac_0005_02_parse_where_and
  - id: AC-0005-03
    given: la consulta "SELECT * FROM t LIMIT 10"
    when: se parsea
    then: produce limit Some(10)
    test: test_ac_0005_03_parse_limit
  - id: AC-0005-04
    given: la consulta invalida "SELECT FROM"
    when: se parsea
    then: devuelve ParseError con posicion
    test: test_ac_0005_04_parse_error_has_position
  - id: AC-0005-05
    given: una consulta valida
    when: se hace Display y se vuelve a parsear
    then: el AST resultante es igual (roundtrip)
    test: test_ac_0005_05_display_parse_roundtrip
exit_criteria:
  - cargo test -p ruscadb-query -- test_ac_0005
  - cargo mutants -p ruscadb-query mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir ruscadb-query al stub
sandbox:
  - cargo test -p ruscadb-query
---

# SPEC-0005 — Lenguaje de consulta (RQL) y parser

## Contexto

Primer componente del query engine (`docs/RuscaDB-roadmap.md` §4 y ADR-001/007).
Se implementa un **lexer + parser recursive-descent a mano** para un subconjunto
RQL (`SELECT ... FROM ... WHERE ... LIMIT`), produciendo un IR tipado. La
integración con DataFusion/sqlparser-rs (dialecto completo, grafos/vectores)
queda como ruta posterior.

## Criterios de aceptación

- **AC-0005-01..03** — parseo de SELECT/proyección/WHERE+AND/LIMIT.
- **AC-0005-04** — errores accionables con posición (WHAT+WHERE).
- **AC-0005-05** — roundtrip `Display → parse` (propiedad metamórfica).

## Trazabilidad

Tests `test_ac_0005_<nn>_*` + property tests; verificado por `cargo xtask trace`.
