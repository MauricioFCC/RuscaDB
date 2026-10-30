---
id: SPEC-0111
feature: onnx_inference
status: implemented
maturity: real
owner: ai-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-ai]
  files: [crates/ruscadb/src/onnx.rs]
  out_of_scope: [entrenamiento, modelos grandes, GPU]
fr:
  - { id: FR-0111-01, desc: "feature flag `onnx` (off por defecto) habilita inferencia ONNX" }
  - { id: FR-0111-02, desc: "OnnxModel::load y OnnxModel::infer producen un vector de f32" }
nf:
  - { id: NF-0111-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0111-01
    given: la feature `onnx` activada
    when: se crea un OnnxModel con un path
    then: load no paniquea y devuelve Ok o error tipado
    test: test_ac_0111_01_onnx_load_safe
exit_criteria:
  - cargo test -p ruscadb --all-features --test onnx
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0111 â€” Inferencia ONNX en la base

## Contexto

Frontier 2026 (SurrealDB): inferencia ML integrada en la base. RuscaDB tiene
embeddings estÃ¡ticos (SPEC-0083, model2vec). Esta spec aÃ±ade inferencia ONNX
opcional detrÃ¡s de un feature flag, complementando los embeddings estÃ¡ticos.

## Superficie

- Feature `onnx` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/onnx.rs` â€” `OnnxModel` con `load` + `infer`.
- IntegraciÃ³n futura: UDF en RQL (`EMBED(text)`), hybrid search con re-ranking.

## Notas

- ImplementaciÃ³n inicial: stub sin dependencia de `ort` (no se aÃ±ade al build
  por defecto). La integraciÃ³n real con `ort` queda como trabajo futuro cuando
  se requiera inferencia transformer.
---
