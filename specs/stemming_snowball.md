---
id: SPEC-0144
feature: fts_stemming_snowball
status: implemented
maturity: stub
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-fts]
  files: [crates/ruscadb/src/stemming.rs]
  out_of_scope: [modelos de lenguaje, embeddings semÃ¡nticos]
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

# SPEC-0144 â€” Stemming Snowball (multi-idioma)

## Contexto

Frontier 2026 (Elasticsearch, Lucene): stemming Snowball para mÃºltiples idiomas.
RuscaDB tiene stemming bÃ¡sico (Porter-like, SPEC-0117). Esta spec aÃ±ade Snowball
detrÃ¡s de un feature flag.

## Superficie

- Feature `stemming` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/stemming.rs` â€” `SnowballStemmer` con `stem`.
- IntegraciÃ³n futura: stemming en RQL `MATCH`, multi-idioma.

## Estado del arte

- **Snowball**: algoritmo de stemming para 15+ idiomas.
- **Lucene**: SnowballStemmer con soporte multi-idioma.
- **RuscaDB actual**: stemming bÃ¡sico Porter-like (SPEC-0117, inglÃ©s).

## DiseÃ±o propuesto

1. **SnowballStemmer**: implementaciÃ³n del algoritmo Snowball.
2. **Idiomas**: inglÃ©s, espaÃ±ol, francÃ©s, alemÃ¡n, etc.
3. **IntegraciÃ³n**: stemming en el Ã­ndice invertido (FTS).

## Notas

- ImplementaciÃ³n inicial: stemming inglÃ©s (Porter/Snowball).
- Multi-idioma queda como trabajo futuro.
---
