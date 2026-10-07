---
id: SPEC-0015
feature: query_extensions
status: accepted
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb-query, ruscadb-core]
  out_of_scope: [ejecucion en la fachada, DataFusion, JOIN, agregaciones]
fr:
  - { id: FR-0015-01, desc: "parsear KNN: SELECT ... FROM t KNN <col> <|k|> [v1, v2, ...]" }
  - { id: FR-0015-02, desc: "parsear TRAVERSE: SELECT ... FROM t TRAVERSE <col> DEPTH <n>" }
  - { id: FR-0015-03, desc: "parsear EXPLAIN <select> como envoltura del IR" }
  - { id: FR-0015-04, desc: "Display de las nuevas formas (roundtrip Display->parse estable)" }
  - { id: FR-0015-05, desc: "errores accionables con posicion ante KNN/TRAVERSE mal formados" }
nf:
  - { id: NF-0015-01, desc: "parse O(n) y sin panics ante entrada arbitraria (proptest/fuzz)" }
  - { id: NF-0015-02, desc: "las formas previas (SELECT/WHERE/LIMIT) siguen intactas (sin regresion)" }
acceptance_criteria:
  - id: AC-0015-01
    given: "SELECT * FROM docs KNN embedding <|5|> [0.1, 0.2, 0.3]"
    when: se parsea
    then: produce un Select con knn Some{column:"embedding", k:5, query:[0.1,0.2,0.3]}
    test: test_ac_0015_01_parse_knn
  - id: AC-0015-02
    given: "SELECT * FROM nodes TRAVERSE edges DEPTH 3"
    when: se parsea
    then: produce un Select con traverse Some{column:"edges", depth:3}
    test: test_ac_0015_02_parse_traverse
  - id: AC-0015-03
    given: "EXPLAIN SELECT * FROM t WHERE a = 1"
    when: se parsea
    then: produce un Explain que envuelve el Select interno
    test: test_ac_0015_03_parse_explain
  - id: AC-0015-04
    given: una consulta con KNN/TRAVERSE
    when: se hace Display y se reparsea
    then: el IR resultante es igual (roundtrip)
    test: test_ac_0015_04_display_parse_roundtrip_extensions
  - id: AC-0015-05
    given: "SELECT * FROM t KNN embedding <|x|> []" (k invalido) o TRAVERSE sin DEPTH
    when: se parsea
    then: devuelve ParseError con posicion
    test: test_ac_0015_05_malformed_extensions_are_errors
exit_criteria:
  - cargo test -p ruscadb-query -- test_ac_0015
  - cargo mutants -p ruscadb-query mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir ruscadb-query al parser previo (SPEC-0005)
sandbox:
  - cargo test -p ruscadb-query
---

# SPEC-0015 — Extensiones del lenguaje RQL (KNN, TRAVERSE, EXPLAIN)

## Contexto

El roadmap (§4.6 ADR-007) define un único lenguaje con extensiones para grafos y
vectores (`->`, `KNN`, `TRAVERSE`). Esta spec extiende el IR y el parser
(SPEC-0005) con tres formas **sin cambiar la ejecución** (que se integra en la
fachada en una iteración posterior):

- `KNN <columna> <|k|> [v1, v2, ...]` — búsqueda ANN declarativa en el `Select`.
- `TRAVERSE <columna> DEPTH <n>` — traversal de grafo declarativo.
- `EXPLAIN <select>` — envoltura del IR para inspección del plan.

## Criterios de aceptación

- **AC-0015-01..03** — parseo de las tres formas.
- **AC-0015-04** — roundtrip `Display → parse`.
- **AC-0015-05** — errores con posición ante formas mal formadas.

## Trazabilidad

Tests `test_ac_0015_<nn>_*` + proptests; verificado por `cargo xtask trace`.
