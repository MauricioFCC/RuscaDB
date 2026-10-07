---
id: SPEC-0014
feature: full_text_search
status: accepted
owner: search-team
appetite_days: 8
boundaries:
  crates: [ruscadb-fts, ruscadb-core]
  out_of_scope: [persistencia en paginas, integracion en executor, stemming avanzado, sinonimos]
fr:
  - { id: FR-0014-01, desc: "tokenizador Unicode: segmenta por limites de palabra, minusculas, sin stopwords por defecto" }
  - { id: FR-0014-02, desc: "indice invertido: term -> postings (doc_id, tf) con orden estable" }
  - { id: FR-0014-03, desc: "ranking BM25 (k1=1.2, b=0.75) sobre consulta multi-termino (OR de terminos)" }
  - { id: FR-0014-04, desc: "estadisticas del corpus: N docs, doc_len, avgdl, df por termino" }
  - { id: FR-0014-05, desc: "borrado logico de documentos (tombstone) excluido del ranking" }
  - { id: FR-0014-06, desc: "serializacion serde del indice (roundtrip estable)" }
nf:
  - { id: NF-0014-01, desc: "search O(sum df) sobre terminos de la consulta (sin recorrer todos los docs)" }
  - { id: NF-0014-02, desc: "sin panics ante entrada arbitraria (proptest)" }
  - { id: NF-0014-03, desc: "determinista: mismo corpus + consulta => mismo ranking" }
acceptance_criteria:
  - id: AC-0014-01
    given: el documento "the quick brown fox"
    when: se tokeniza
    then: produce [the, quick, brown, fox] en minusculas
    test: test_ac_0014_01_tokenize_lowercases_and_splits
  - id: AC-0014-02
    given: dos documentos con distintos terminos
    when: se busca un termino
    then: solo aparecen los documentos que lo contienen
    test: test_ac_0014_02_inverted_index_returns_matching_docs
  - id: AC-0014-03
    given: tres documentos con distinta relevancia para "gato"
    when: se busca "gato"
    then: el ranking BM25 ordena el documento mas relevante primero
    test: test_ac_0014_03_bm25_ranks_relevant_first
  - id: AC-0014-04
    given: un indice con documentos
    when: se serializa y deserializa
    then: el indice reconstruido da el mismo ranking
    test: test_ac_0014_04_serde_roundtrip_preserves_ranking
  - id: AC-0014-05
    given: un documento borrado
    when: se busca un termino que contenia
    then: el documento no aparece en los resultados
    test: test_ac_0014_05_deleted_docs_are_excluded
exit_criteria:
  - cargo test -p ruscadb-fts -- test_ac_0014
  - cargo mutants -p ruscadb-fts mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - eliminar el crate ruscadb-fts del workspace
sandbox:
  - cargo test -p ruscadb-fts
---

# SPEC-0014 — Full-text search (índice invertido + BM25)

## Contexto

Tercer modelo de índice del roadmap (§5.4, F3). Se implementa un índice
invertido con ranking **BM25** (`k1 = 1.2`, `b = 0.75`) en un crate aislado
`ruscadb-fts`, sin dependencias externas de tokenización (tokenizador Unicode
propio, determinista y auditable). La integración en el executor
(`... WHERE MATCH(col, 'consulta')`) y la persistencia en páginas quedan fuera
de esta spec (siguiente iteración).

## Criterios de aceptación

- **AC-0014-01** — tokenizador (Unicode, minúsculas).
- **AC-0014-02** — índice invertido devuelve solo documentos coincidentes.
- **AC-0014-03** — BM25 ordena por relevancia.
- **AC-0014-04** — roundtrip serde estable.
- **AC-0014-05** — borrado lógico excluye del ranking.

## Trazabilidad

Tests `test_ac_0014_<nn>_*` + proptests (tokenización sin panics, ranking
determinista); verificado por `cargo xtask trace`.
