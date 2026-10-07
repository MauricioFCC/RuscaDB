---
id: SPEC-0023
feature: wasm_bindings
status: accepted
owner: platform-team
appetite_days: 6
boundaries:
  crates: [ruscadb-wasm, ruscadb-query, ruscadb-vector, ruscadb-fts, ruscadb-fvs, ruscadb-crypto, ruscadb-core]
  out_of_scope: [persistencia en WASM, OPFS/IndexedDB, hilos]
fr:
  - { id: FR-0023-01, desc: "parse_query(sql) -> JSON del IR (Statement) con errores como JSON" }
  - { id: FR-0023-02, desc: "vector_search(corpus, query, k) en JSON usando distancia/HNSW" }
  - { id: FR-0023-03, desc: "text_search(docs, query, k) en JSON usando BM25" }
  - { id: FR-0023-04, desc: "seal/open de bytes en base64/hex (cifrado en reposo en el cliente)" }
  - { id: FR-0023-05, desc: "la capa JS (wasm-bindgen) vive tras la feature `wasm`" }
nf:
  - { id: NF-0023-01, desc: "sin panics: toda entrada invalida devuelve un JSON de error" }
  - { id: NF-0023-02, desc: "compila y se testea en el host sin la feature `wasm`" }
acceptance_criteria:
  - id: AC-0023-01
    given: una query RQL valida
    when: se llama parse_query
    then: devuelve el JSON del Statement con las clausulas esperadas
    test: test_ac_0023_01_parse_query_json
  - id: AC-0023-02
    given: un corpus de vectores y una query
    when: se llama vector_search
    then: devuelve los k vecinos en JSON ordenados por distancia
    test: test_ac_0023_02_vector_search_json
  - id: AC-0023-03
    given: documentos de texto y una consulta
    when: se llama text_search
    then: devuelve los documentos ordenados por BM25 en JSON
    test: test_ac_0023_03_text_search_json
  - id: AC-0023-04
    given: una clave y bytes
    when: se llama seal y luego open
    then: se recuperan los bytes originales (roundtrip)
    test: test_ac_0023_04_crypto_roundtrip
  - id: AC-0023-05
    given: una query invalida o un JSON malformado
    when: se llama la API
    then: devuelve un JSON de error accionable (sin panics)
    test: test_ac_0023_05_errors_are_json
exit_criteria:
  - cargo test -p ruscadb-wasm -- test_ac_0023
  - cargo check -p ruscadb-wasm --features wasm
  - cargo mutants -p ruscadb-wasm mutation score >= 70%
rollback:
  - eliminar el crate ruscadb-wasm del workspace
sandbox:
  - cargo test -p ruscadb-wasm
---

# SPEC-0023 — Bindings WebAssembly

## Contexto

Completa F5 (bindings) con el objetivo **WASM** del roadmap (§4.3, ADR-011):
RuscaDB es in-process y sin red, apto para el navegador. El crate `ruscadb-wasm`
expone una API **serializable en JSON** sobre los componentes puros del motor
(parser RQL, búsqueda vectorial, full-text, cifrado), y la capa `wasm-bindgen`
vive tras la feature `wasm` para poder compilar/testear en el host sin el target
wasm32.

## Criterios de aceptación

- **AC-0023-01..04** — parse_query / vector_search / text_search / crypto.
- **AC-0023-05** — errores como JSON, sin panics.

## Trazabilidad

Tests `test_ac_0023_<nn>_*` + proptests; verificado por `cargo xtask trace`.
