---
id: SPEC-0049
feature: ai_embedder
status: accepted
owner: ai-team
appetite_days: 6
boundaries:
  crates: [ruscadb-ai]
  out_of_scope: [descarga de modelos, GPU, tokenizadores BPE]
fr:
  - { id: FR-0049-01, desc: "TfIdfEmbedder local (n-gramas de palabras) con vocabulario construido por fit" }
  - { id: FR-0049-02, desc: "embed deterministico, L2-normalizado, dim configurable" }
  - { id: FR-0049-03, desc: "ruta ONNX feature-gated (`onnx`) documentada (no compilada por defecto)" }
  - { id: FR-0049-04, desc: "el trait Embedder se mantiene; HashingEmbedder y TfIdfEmbedder lo implementan" }
nf:
  - { id: NF-0049-01, desc: "determinista y sin panics; textos vacios => vector cero valido" }
  - { id: NF-0049-02, desc: "sin nuevas dependencias obligatorias (ONNX solo tras la feature)" }
acceptance_criteria:
  - id: AC-0049-01
    given: un corpus de entrenamiento
    when: se hace fit y luego embed
    then: el vector tiene la dimension configurada y esta L2-normalizado
    test: test_ac_0049_01_tfidf_dim_and_norm
  - id: AC-0049-02
    given: dos textos con solape de terminos
    when: se comparan sus embeddings
    then: la similitud coseno es mayor que para textos sin solape
    test: test_ac_0049_02_tfidf_similarity
  - id: AC-0049-03
    given: el mismo texto
    when: se embebe dos veces
    then: el vector es identico (determinista)
    test: test_ac_0049_03_deterministic
  - id: AC-0049-04
    given: un texto vacio o desconocido
    when: se embebe
    then: devuelve un vector cero valido (sin panics)
    test: test_ac_0049_04_empty_text
  - id: AC-0049-05
    given: la feature `onnx` desactivada
    when: se compila el crate
    then: compila sin dependencias de ONNX (documentado)
    test: test_ac_0049_05_onnx_feature_documented
exit_criteria:
  - cargo test -p ruscadb-ai -- test_ac_0049
  - cargo mutants -p ruscadb-ai mutation score >= 70%
rollback:
  - revertir russcadb-ai
sandbox:
  - cargo test -p ruscadb-ai
---

# SPEC-0049 — Embedder local TF-IDF + ruta ONNX documentada

## Contexto

F4 pide embeddings reales. Sin descargar modelos, se añade `TfIdfEmbedder`
(n-gramas de palabras, vocabulario por `fit`, L2-normalizado, determinista) y se
documenta una ruta ONNX feature-gated (sin dependencias por defecto). El trait
`Embedder` se mantiene.

## Criterios de aceptación

- **AC-0049-01/02/03** — dim/norma, similitud, determinismo.
- **AC-0049-04/05** — texto vacío y feature ONNX documentada.

## Trazabilidad

Tests `test_ac_0049_<nn>_*`; verificado por `cargo xtask trace`.
