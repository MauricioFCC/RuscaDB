---
id: SPEC-0044
feature: document_operators
status: accepted
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb-query, ruscadb]
  out_of_scope: [indices GIN sobre JSON, JSONPath completo, arrays indexados]
fr:
  - { id: FR-0044-01, desc: "col -> 'a' y col -> 'a.b' extraen un campo del documento JSON (Record.doc)" }
  - { id: FR-0044-02, desc: "col @> '{json}' comprueba contencion del documento" }
  - { id: FR-0044-03, desc: "los operadores se usan en WHERE con comparaciones (=, >, <, ...)" }
  - { id: FR-0044-04, desc: "Display/roundtrip de ambos operadores" }
nf:
  - { id: NF-0044-01, desc: "sin regresion en queries sin operadores; O(1)/O(profundidad) por fila" }
  - { id: NF-0044-02, desc: "documento ausente/campo ausente => la fila no cumple (no error); JSON invalido en @> => error accionable" }
acceptance_criteria:
  - id: AC-0044-01
    given: filas con doc {a: 1} y {a: 2}
    when: se ejecuta "SELECT * FROM t WHERE doc -> 'a' = 1"
    then: devuelve solo la fila con a=1
    test: test_ac_0044_01_doc_extract_equals
  - id: AC-0044-02
    given: docs anidados {nested: {n: 5}}
    when: se ejecuta "WHERE doc -> 'nested.n' > 3"
    then: devuelve la fila (extraccion por ruta anidada)
    test: test_ac_0044_02_doc_extract_nested
  - id: AC-0044-03
    given: docs {tags: ["gato"]}
    when: se ejecuta "WHERE doc @> '{\"tags\":[\"gato\"]}'"
    then: devuelve la fila que contiene el subdocumento
    test: test_ac_0044_03_doc_contains
  - id: AC-0044-04
    given: un documento sin el campo
    when: se consulta con ->
    then: la fila no cumple (no error, no panic)
    test: test_ac_0044_04_missing_field_excludes_row
  - id: AC-0044-05
    given: las dos formas
    when: se hace Display y se reparsean
    then: el IR es igual (roundtrip)
    test: test_ac_0044_05_display_parse_roundtrip_doc_ops
exit_criteria:
  - cargo test -p ruscadb-query -p ruscadb -- test_ac_0044
  - cargo mutants -p ruscadb-query mutation score >= 70%
rollback:
  - revertir query + fachada
sandbox:
  - cargo test -p ruscadb-query -p ruscadb
---

# SPEC-0044 — Operadores de documento `->` y `@>` (modelo documental)

## Contexto

El modelo documental (`Record.doc: Option<serde_json::Value>`) no era consultable
desde RQL. Se añaden, estilo PostgreSQL: `col -> 'a.b'` (extracción por ruta) y
`col @> '{json}'` (contención), usables en `WHERE` con comparaciones. El
executor accede al `Record` (ya disponible en el scan). Campo/documento ausente
→ la fila no cumple; JSON inválido en `@>` → error accionable.

## Criterios de aceptación

- **AC-0044-01/02/03** — extracción (simple/anidada) y contención.
- **AC-0044-04/05** — campo ausente y roundtrip.

## Trazabilidad

Tests `test_ac_0044_<nn>_*`; verificado por `cargo xtask trace`.
