---
id: SPEC-0172
feature: acorn_filtered_search
status: implemented
maturity: stub
owner: vector-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-vector, ruscadb-fvs]
  files: [crates/ruscadb-fvs/src/lib.rs, crates/ruscadb-vector/src/hnsw.rs]
  out_of_scope: [GPU, distributed]
fr:
  - { id: FR-0172-01, desc: "ACORN: busqueda predicado-agnostica sobre HNSW" }
  - { id: FR-0172-02, desc: "expansion de vecindario para mantener recall con filtros selectivos" }
  - { id: FR-0172-03, desc: "integracion con iFVS (pre/in/post)" }
nf:
  - { id: NF-0172-01, desc: "recall@10 >= 0.95 con selectividad 1%" }
acceptance_criteria:
  - id: AC-0172-01
    given: una busqueda vectorial con filtro selectivo (1%)
    when: se ejecuta con ACORN
    then: recall@10 >= 0.95
    test: test_ac_0172_01_recall
  - id: AC-0172-02
    given: un filtro no selectivo (50%)
    when: se ejecuta
    then: se usa la ruta pre-filter (iFVS) por costo
    test: test_ac_0172_02_prefilter
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-fvs --all-features
  - cargo test -p ruscadb-vector --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0172 — ACORN: filtered vector search predicado-agnostico

## Contexto

iFVS (SPEC-0012) elige estrategia por selectividad. Con filtros muy
selectivos, HNSW pierde recall porque los vecinos filtrados son pocos.
ACORN expande el vecindario para mantener recall.

## Superficie

- `fvs/lib.rs`: `AcornSearch { search, expand_factor }`.
- `hnsw.rs`: soporte de expansion de vecindario.

## Estado del arte

- **ACORN** (SIGMOD 2024): "Performant and Predicate-Agnostic Search Over
  Vector Embeddings and Structured Data".
- **Filtered-DiskANN** (2023): filtered search en disco.
- **RuscaDB actual**: iFVS (pre/in/post) sin expansion.

## Diseño propuesto

1. **Expansion**: al buscar, expandir el beam search por factor k cuando el
   filtro elimina vecinos.
2. **Adaptativo**: si selectividad < threshold, usar ACORN; si no, iFVS normal.
3. **Rerank**: candidatos expandidos se filtran y re-rankean.

## Riesgos

- Latencia: expansion aumenta trabajo; tradeoff recall/latencia.

## ADR requerido

`docs/adr/ADR-0172-acorn.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\acorn_filtered_search.md