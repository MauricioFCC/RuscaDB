---
id: SPEC-0083
feature: static_embeddings
status: implemented
owner: ai-team
appetite_days: 3
boundaries:
  crates: [ruscadb-ai]
  files: [crates/ruscadb-ai/Cargo.toml, crates/ruscadb-ai/src/lib.rs, crates/ruscadb-ai/tests/static_embeddings.rs]
  out_of_scope: [runtimes neuronales (Candle/ONNX siguen OFF, ADR-016), descarga de modelos en runtime, cross-encoder, fine-tuning, GPU]
fr:
  - { id: FR-0083-01, desc: "feature `static-embeddings` (default OFF) con la crate `model2vec-rs` cargando pesos safetensors LOCALES (p.ej. potion-base-8M); nunca descarga en runtime" }
  - { id: FR-0083-02, desc: "StaticEmbedder implementa el trait Embedder existente: fija dim, normaliza, es determinista y encaja en el cableado actual de la fachada" }
  - { id: FR-0083-03, desc: "el build por defecto NO compila model2vec-rs (cero coste, cero peso para quien no lo pide)" }
nf:
  - { id: NF-0083-01, desc: "tests sin red: fixture safetensors mínimo generado en el propio test (el modelo real de ~30 MB es criterio de aceptación manual, no de CI)" }
  - { id: NF-0083-02, desc: "enmienda documentada a ADR-016: pesos estáticos SÍ, runtimes NO (el ADR vive en la tabla §4.6 del roadmap, edición del coordinador)" }
  - { id: NF-0083-03, desc: "docstrings ES-UTF8; sin unwrap en producción; errores WHAT+WHY+WHERE (modelo no encontrado => error accionable)" }
acceptance_criteria:
  - id: AC-0083-01
    given: un fixture safetensors mínimo en disco (local-only)
    when: se construye StaticEmbedder con la feature activa y se embedde un texto
    then: devuelve el vector fijo del fixture con la dim declarada, sin red
    test: test_ac_0083_01_static_embedder_loads_local_fixture
  - id: AC-0083-02
    given: el código fuente de ruscadb-ai
    when: se comprueba el gating de la feature
    then: model2vec-rs solo aparece detrás de cfg(feature = "static-embeddings") y el default sigue usando los embedders puros (test sin la feature)
    test: test_ac_0083_02_default_build_stays_dependency_free
  - id: AC-0083-03
    given: StaticEmbedder construido dos veces desde el mismo fixture
    when: se embedde el mismo texto
    then: vectores idénticos byte a byte (determinismo) y de dim constante
    test: test_ac_0083_03_static_embedder_is_deterministic
exit_criteria:
  - cargo test -p ruscadb-ai --test static_embeddings --features static-embeddings
  - cargo test -p ruscadb-ai                (default, sin feature)
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - retirar la feature y StaticEmbedder (vuelven solo HashingEmbedder/TfIdfEmbedder)
sandbox:
  - cargo test -p ruscadb-ai --test static_embeddings --features static-embeddings
---

# SPEC-0083 — Embeddings estáticos `model2vec-rs` (Ola 4, enmienda ADR-016)

## Contexto

Auditoría *readiness* (riesgo 4): la «IA real» es scaffolding —
`HashingEmbedder`/`TfIdfEmbedder` puros y `onnx = []` vacío (SPEC-0049
reservó la feature sin implementar). Sin runtime neuronal (ADR-016), la vía
de frontera para calidad semántica real es **embeddings estáticos**:
`model2vec` (arXiv:2410.22517) con pesos `potion-base-8M` (~30 MB,
safetensors, dim 256, local-only) — sin ONNX, sin Candle, sin GPU.

**Enmienda ADR-016**: pesos estáticos SÍ (modelo embebido/archivo local),
runtimes NO (la exclusión de Candle/ONNX se mantiene).

## Superficie

- `crates/ruscadb-ai/Cargo.toml`: `model2vec-rs` opcional tras
  `[features] static-embeddings = ["dep:model2vec-rs"]` (default OFF).
- `crates/ruscadb-ai/src/lib.rs`: `StaticEmbedder` (carga safetensors de
  ruta local, error accionable si no existe), detrás de
  `#[cfg(feature = "static-embeddings")]`.
- Test: `crates/ruscadb-ai/tests/static_embeddings.rs` con fixture
  safetensors mínimo generado por el propio test (sin red, sin modelo real).

## Criterios de aceptación

- **AC-0083-01** — carga fixture local y embedde (feature ON).
- **AC-0083-02** — default sin la feature: sin dependencia nueva (test cfg negado).
- **AC-0083-03** — determinismo byte a byte y dim constante.

## Trazabilidad

Tests `test_ac_0083_<nn>_*` con marcador `@spec AC-0083-NN`; verificado por
`cargo xtask trace`/`contract`.
