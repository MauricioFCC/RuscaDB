---
id: SPEC-0125
feature: ivf_pq
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-vector]
  files: [crates/ruscadb/src/ivf_pq.rs]
  out_of_scope: [GPU acceleration, distributed search]
fr:
  - { id: FR-0125-01, desc: "feature flag `ivf-pq` (off por defecto) habilita IVF-PQ" }
  - { id: FR-0125-02, desc: "IvfPqIndex::quantize reduce dimension por factor" }
nf:
  - { id: NF-0125-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0125-01
    given: la feature `ivf-pq` activada
    when: se cuantiza un vector
    then: el resultado tiene menos dimensiones
    test: test_ac_0125_01_ivf_pq_quantize
exit_criteria:
  - cargo test -p ruscadb --all-features --test ivf_pq
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0125 — IVF-PQ para vectores grandes

## Contexto

Frontier 2026 (LanceDB, ParqDB): IVF-PQ reduce memoria 4-8x para datasets >1M.
RuscaDB usa HNSW puro. Esta spec añade IVF-PQ básico detrás de un feature flag.

## Superficie

- Feature `ivf-pq` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/ivf_pq.rs` — `IvfPqIndex` con `quantize`.

## Notas

- Implementación inicial: cuantización por subespacios (PQ) sin IVF clustering.
- IVF clustering y búsqueda ANN quedan como trabajo futuro.
---
