---
id: SPEC-0171
feature: rabitq_quantization
status: implemented
owner: vector-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-vector]
  files: [crates/ruscadb-vector/src/quant.rs, crates/ruscadb/src/ivf_pq.rs]
  out_of_scope: [GPU, distributed search]
fr:
  - { id: FR-0171-01, desc: "RaBitQ: cuantizacion 1-bit por dimension con bound teorico de error" }
  - { id: FR-0171-02, desc: "estimacion de distancia con correccion de sesgo" }
  - { id: FR-0171-03, desc: "integracion con HNSW como storage comprimido" }
nf:
  - { id: NF-0171-01, desc: "reduccion de memoria ~32x vs f32" }
  - { id: NF-0171-02, desc: "recall@10 >= 0.90 con rerank exacto" }
acceptance_criteria:
  - id: AC-0171-01
    given: un vector de dimension d
    when: se cuantiza con RaBitQ
    then: el estimador de distancia es insesgado (error acotado)
    test: test_ac_0171_01_encode
  - id: AC-0171-02
    given: una busqueda HNSW con storage RaBitQ
    when: se ejecuta con rerank exacto top-100
    then: recall@10 >= 0.90
    test: test_ac_0171_02_recall
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-vector --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0171 — RaBitQ quantization para HNSW

## Contexto

HNSW almacena vectores f32 (4 bytes/dim). Para datasets grandes la memoria
es el cuello de botella. RaBitQ ofrece cuantizacion 1-bit con bound teorico
de error y estimador insesgado.

## Superficie

- `quant.rs`: `RaBitQuantizer { encode, decode, estimate_distance }`.
- `ivf_pq.rs` / HNSW: storage comprimido + rerank exacto.

## Estado del arte

- **RaBitQ** (SIGMOD 2024): "Quantizing High-Dimensional Vectors with a
  Theoretical Error Bound for Approximate Nearest Neighbor Search".
- **TurboQuant** (2025): quantizacion extrema con online codebook.
- **RuscaDB actual**: HNSW f32 (SPEC-0006), IVF-PQ stub (SPEC-0125).

## Diseño propuesto

1. **Encode**: centrar vector, random rotation (Hadamard), signo por dim.
2. **Estimador**: distancia estimada con correccion de sesgo (factor).
3. **Rerank**: top-k candidatos se re-evaluan con vectores exactos.

## Riesgos

- Random rotation: costo O(d log d) por vector; aceptable en ingesta.

## ADR requerido

`docs/adr/ADR-0171-rabitq.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\rabitq_quantization.md