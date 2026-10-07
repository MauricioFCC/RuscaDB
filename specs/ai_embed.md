---
id: SPEC-0009
feature: ai_embed
status: accepted
owner: ai-team
appetite_days: 5
boundaries:
  crates: [ruscadb-ai, ruscadb-core]
  out_of_scope: [storage, query, bindings]
fr:
  - { id: FR-0009-01, desc: "puerto Embedder (trait) con embed(text) -> Vec<f32>" }
  - { id: FR-0009-02, desc: "embedder local determinista (hashing trick / proyeccion) sin GPU" }
  - { id: FR-0009-03, desc: "salida L2-normalizada y de dimension configurable" }
nf:
  - { id: NF-0009-01, desc: "embed determinista y reproducible (mismo texto => mismo vector)" }
  - { id: NF-0009-02, desc: "sin dependencias pesadas (Candle/ONNX quedan como feature futura)" }
acceptance_criteria:
  - id: AC-0009-01
    given: el mismo texto embebido dos veces
    when: se comparan los vectores
    then: son identicos (determinismo)
    test: test_ac_0009_01_embedding_is_deterministic
  - id: AC-0009-02
    given: un embedder de dimension d
    when: se embebe un texto
    then: el vector tiene exactamente d componentes
    test: test_ac_0009_02_embedding_dimension
  - id: AC-0009-03
    given: dos textos distintos
    when: se embeben
    then: los vectores difieren
    test: test_ac_0009_03_distinct_texts_differ
  - id: AC-0009-04
    given: un vector embebido no nulo
    when: se calcula su norma L2
    then: es ~1.0 (normalizado)
    test: test_ac_0009_04_embedding_is_normalized
exit_criteria:
  - cargo test -p ruscadb-ai -- test_ac_0009
  - cargo mutants -p ruscadb-ai mutation score >= 70%
rollback:
  - revertir ruscadb-ai al stub
sandbox:
  - cargo test -p ruscadb-ai
---

# SPEC-0009 — Puerta de embeddings (IA local)

## Contexto

Puerto `Embedder` de RuscaDB (`docs/RuscaDB-roadmap.md` §6, ADR-005). Se
implementa un embedder local **determinista** (hashing trick con proyección
firmada) para no bloquear el desarrollo con dependencias pesadas; Candle/ONNX
quedan como feature futura.

## API esperada

`trait Embedder { fn dim(&self) -> usize; fn embed(&self, text: &str) -> Vec<f32>; }`
y `HashingEmbedder::new(dim) -> Result<Self, RuscaError>`.

## Trazabilidad

Tests `test_ac_0009_<nn>_*`; verificado por `cargo xtask trace`.
