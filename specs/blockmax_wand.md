---
id: SPEC-0177
feature: blockmax_wand
status: proposed
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-fts]
  files: [crates/ruscadb-fts/src/index.rs, crates/ruscadb-fts/src/bm25.rs]
  out_of_scope: [distributed search, neural reranking]
fr:
  - { id: FR-0177-01, desc: "BlockMax WAND para top-k eficiente" }
  - { id: FR-0177-02, desc: "max score por bloque para pruning" }
  - { id: FR-0177-03, desc: "integracion con BM25 (SPEC-0014)" }
nf:
  - { id: NF-0177-01, desc: "cero dependencias nuevas" }
  - { id: NF-0177-02, desc: "speedup >= 5x vs scan completo en top-10" }
acceptance_criteria:
  - id: AC-0177-01
    given: un indice FTS con N documentos
    when: se ejecuta top-10 BM25
    then: se devuelven los 10 mejores sin scan completo
  - id: AC-0177-02
    given: un termino raro (baja df)
    when: se ejecuta
    then: el pruning es efectivo (pocos bloques evaluados)
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-fts --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0177 — BlockMax WAND para top-k FTS

## Contexto

FTS (SPEC-0014) usa BM25 con scan completo. Para indices grandes, top-k
requiere evaluar todos los documentos. BlockMax WAND poda bloques cuyo score
maximo no puede entrar al top-k.

## Superficie

- `index.rs`: postings con max-score por bloque (BlockMax).
- `bm25.rs`: `top_k(query, k) -> Vec<(DocId, f32)>`.

## Estado del arte

- **WAND** (Broder et al., 2003): Weak AND para top-k.
- **BlockMax WAND** (Ding & Suel, WSDM 2011): pruning por bloque.
- **Lucene**: BlockMax WAND por defecto.
- **RuscaDB actual**: BM25 scan completo (SPEC-0014).

## Diseño propuesto

1. **Postings**: listas de (docid, tf) agrupadas en bloques de 128 docs;
   cada bloque almacena max tf (y max score precomputado).
2. **WAND**: mantener threshold del k-esimo mejor; saltar bloques cuyo
   max score <= threshold.
3. **Rerank**: evaluar documentos de bloques no podados.

## Riesgos

- Peor caso (query con terminos muy frecuentes): degenera a scan.

## ADR requerido

`docs/adr/ADR-0177-blockmax-wand.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\blockmax_wand.md