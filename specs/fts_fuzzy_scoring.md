---
id: SPEC-0123
feature: fts_fuzzy_scoring
status: implemented
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-fts]
  files: [crates/ruscadb/src/fts_scoring.rs]
  out_of_scope: [modelos de lenguaje, embeddings semánticos]
fr:
  - { id: FR-0123-01, desc: "feature flag `fts-scoring` (off por defecto) habilita scoring fuzzy" }
  - { id: FR-0123-02, desc: "FuzzyScorer::score combina BM25 con distancia Levenshtein" }
nf:
  - { id: NF-0123-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0123-01
    given: la feature `fts-scoring` activada
    when: se calcula el score fuzzy de un documento
    then: el score es mayor para documentos más similares
    test: test_ac_0123_01_fuzzy_score
exit_criteria:
  - cargo test -p ruscadb --all-features --test fts_scoring
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0123 — Fuzzy scoring en FTS

## Contexto

Frontier 2026 (Elasticsearch): scoring fuzzy que combina BM25 con distancia de
edición. RuscaDB tiene BM25 (SPEC-0009) y fuzzy matching (SPEC-0117). Esta spec
añade scoring combinado detrás de un feature flag.

## Superficie

- Feature `fts-scoring` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/fts_scoring.rs` — `FuzzyScorer` con `score`.
- Integración futura: scoring en RQL `MATCH ... FUZZY`.

## Notas

- Implementación inicial: combinación lineal de BM25 y similitud Levenshtein.
- Scoring avanzado (neural, learning-to-rank) queda como trabajo futuro.
---
