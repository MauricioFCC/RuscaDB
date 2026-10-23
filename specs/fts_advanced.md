---
id: SPEC-0117
feature: fts_advanced
status: proposed
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-fts]
  files: [crates/ruscadb/src/fts_advanced.rs]
  out_of_scope: [modelos de lenguaje, embeddings semánticos]
fr:
  - { id: FR-0117-01, desc: "feature flag `fts-advanced` (off por defecto) habilita búsqueda fuzzy y por frases" }
  - { id: FR-0117-02, desc: "FuzzyMatcher::levenshtein calcula distancia de edición" }
  - { id: FR-0117-03, desc: "PhraseMatcher::match_phrase verifica orden de términos" }
nf:
  - { id: NF-0117-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0117-01
    given: la feature `fts-advanced` activada
    when: se calcula la distancia de Levenshtein entre dos cadenas
    then: el resultado es correcto
    test: test_ac_0117_01_levenshtein
  - id: AC-0117-02
    given: la feature `fts-advanced` activada
    when: se busca una frase en un texto
    then: se devuelve true solo si los términos aparecen en orden
    test: test_ac_0117_02_phrase_match
exit_criteria:
  - cargo test -p ruscadb --all-features --test fts_advanced
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0117 — Full-text search avanzado (fuzzy, stemming, frases)

## Contexto

Frontier 2026 (SurrealDB, Elasticsearch): búsqueda fuzzy, stemming y por frases.
RuscaDB tiene BM25 básico (SPEC-0009). Esta spec añade capacidades avanzadas
detrás de un feature flag.

## Superficie

- Feature `fts-advanced` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/fts_advanced.rs` — `FuzzyMatcher`, `PhraseMatcher`.
- Integración futura: stemming (Snowball), fuzzy en RQL (`MATCH ... FUZZY`).

## Notas

- Implementación inicial: Levenshtein + phrase matching sin dependencias.
- Stemming y fuzzy scoring quedan como trabajo futuro.
---
