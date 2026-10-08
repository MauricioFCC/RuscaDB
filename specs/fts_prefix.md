---
id: SPEC-0037
feature: fts_prefix
status: accepted
owner: search-team
appetite_days: 4
boundaries:
  crates: [ruscadb-fts]
  out_of_scope: [phrase queries con posiciones, stemming, fuzzy]
fr:
  - { id: FR-0037-01, desc: "search_prefix(prefix, k): recupera docs cuyos terminos empiezan por el prefijo" }
  - { id: FR-0037-02, desc: "el prefijo se normaliza con el mismo tokenizador (minusculas)" }
  - { id: FR-0037-03, desc: "el ranking combina los postings de todos los terminos del prefijo (BM25)" }
  - { id: FR-0037-04, desc: "prefijo vacio o sin coincidencias => vacio" }
nf:
  - { id: NF-0037-01, desc: "usa el orden del BTreeMap (range scan), no recorre todo el indice" }
  - { id: NF-0037-02, desc: "sin panics; consistente con search exacto cuando el prefijo es un termino completo" }
acceptance_criteria:
  - id: AC-0037-01
    given: docs con terminos que comparten prefijo
    when: se llama search_prefix
    then: recupera todos los docs con terminos del prefijo, ordenados por BM25
    test: test_ac_0037_01_prefix_matches_all
  - id: AC-0037-02
    given: un prefijo que no coincide
    when: se llama search_prefix
    then: devuelve vacio
    test: test_ac_0037_02_prefix_no_match
  - id: AC-0037-03
    given: un prefijo vacio
    when: se llama search_prefix
    then: devuelve vacio sin panics
    test: test_ac_0037_03_empty_prefix
  - id: AC-0037-04
    given: un prefijo igual a un termino completo
    when: se llama search_prefix y search
    then: devuelven los mismos docs (consistencia)
    test: test_ac_0037_04_prefix_of_full_term_consistent
  - id: AC-0037-05
    given: el indice con docs borrados
    when: se llama search_prefix
    then: los tombstones no aparecen
    test: test_ac_0037_05_prefix_excludes_deleted
exit_criteria:
  - cargo test -p ruscadb-fts -- test_ac_0037
  - cargo mutants -p ruscadb-fts mutation score >= 70%
rollback:
  - revertir russcadb-fts
sandbox:
  - cargo test -p ruscadb-fts
---

# SPEC-0037 — Búsqueda por prefijo en FTS (F3)

## Contexto

El full-text BM25 solo soporta términos exactos. Se añade `search_prefix(prefix,
k)` que explota el orden lexicográfico del `BTreeMap` de términos (range scan)
para reunir los postings de todos los términos con ese prefijo y rankear con
BM25, excluyendo tombstones. Consistente con `search` cuando el prefijo es un
término completo.

## Criterios de aceptación

- **AC-0037-01/02/03** — coincidencias, sin coincidencias, vacío.
- **AC-0037-04/05** — consistencia y exclusión de borrados.

## Trazabilidad

Tests `test_ac_0037_<nn>_*`; verificado por `cargo xtask trace`.
