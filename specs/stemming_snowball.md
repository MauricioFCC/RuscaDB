---
id: SPEC-0144
feature: fts_stemming_snowball
status: implemented
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-fts]
  files: [crates/ruscadb/src/stemming.rs]
  out_of_scope: [modelos de lenguaje, embeddings semánticos]
fr:
  - { id: FR-0144-01, desc: "feature flag `stemming` (off por defecto) habilita stemming Snowball" }
  - { id: FR-0144-02, desc: "SnowballStemmer::stem aplica stemming para un idioma" }
nf:
  - { id: NF-0144-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0144-01
    given: la feature `stemming` activada
    when: se stemmea una palabra inglesa
    then: el resultado es la raiz
    test: test_ac_0144_01_stem_english
exit_criteria:
  - cargo test -p ruscadb --all-features --test stemming
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0144 — Stemming Snowball (multi-idioma)

## Contexto

Frontier 2026 (Elasticsearch, Lucene): stemming Snowball para múltiples idiomas.
RuscaDB tiene stemming básico (Porter-like, SPEC-0117). Esta spec añade Snowball
detrás de un feature flag.

## Superficie

- Feature `stemming` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/stemming.rs` — `SnowballStemmer` con `stem`.
- Integración futura: stemming en RQL `MATCH`, multi-idioma.

## Estado del arte

- **Snowball**: algoritmo de stemming para 15+ idiomas.
- **Lucene**: SnowballStemmer con soporte multi-idioma.
- **RuscaDB actual**: stemming básico Porter-like (SPEC-0117, inglés).

## Diseño propuesto

1. **SnowballStemmer**: implementación del algoritmo Snowball.
2. **Idiomas**: inglés, español, francés, alemán, etc.
3. **Integración**: stemming en el índice invertido (FTS).

## Notas

- Implementación inicial: stemming inglés (Porter/Snowball).
- Multi-idioma queda como trabajo futuro.
---
