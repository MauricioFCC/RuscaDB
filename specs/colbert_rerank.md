---
id: SPEC-0072
feature: colbert_rerank
status: implemented
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb]
  out_of_scope: [modelo entrenado/ONNX, path tensorial ColBERTv2 completo, indexacion multi-vector en disco]
fr:
  - { id: FR-0072-01, desc: "late-interaction REAL: MaxSim(q,d) = sum_i max_j cos(q_i, d_j) sobre token-embeddings deterministas por hash (hash embeddings, Svenstrup 2017), sin runtime externo" }
  - { id: FR-0072-02, desc: "el re-scoring del hibrido usa MaxSim en lugar del proxy de overlap; stage MaxSim cuando reordena" }
nf:
  - { id: NF-0072-01, desc: "determinista y reproducible: mismo token -> mismo vector; mismos scores bit a bit; cero red" }
  - { id: NF-0072-02, desc: "cero regresion: degradaciones Bm25Only/KnnOnly y AC-0063/0069/0071 siguen verdes" }
acceptance_criteria:
  - id: AC-0072-01
    given: la consulta "alfa beta gamma" y documentos con 3, 2 y 1 termino de la consulta
    when: se calcula maxsim
    then: el score es estrictamente mayor para el documento con mas terminos cubiertos
    test: test_ac_0072_01_maxsim_rewards_coverage
  - id: AC-0072-02
    given: dos documentos que cubren el mismo conjunto de terminos, uno con repeticiones
    when: se calcula maxsim
    then: la repeticion no infla el score (max por termino, no suma de apariciones)
    test: test_ac_0072_02_maxsim_ignores_repetition
  - id: AC-0072-03
    given: una tabla con textos y vectores donde el orden RRF difiere del orden MaxSim
    when: se ejecuta hybrid_search
    then: el stage es MaxSim y los scores son continuos (no el conteo entero de overlap)
    test: test_ac_0072_03_hybrid_uses_maxsim_scores
  - id: AC-0072-04
    given: el mismo par (consulta, documento)
    when: se calcula maxsim dos veces en procesos/llamadas distintas
    then: el score es identico (determinismo por hash, sin estado)
    test: test_ac_0072_04_maxsim_is_deterministic
  - id: AC-0072-05
    given: una tabla sin columna FTS o sin indice vectorial
    when: se ejecuta hybrid_search
    then: degrada igual que antes (Bm25Only/KnnOnly) sin invocar MaxSim
    test: test_ac_0072_05_degradation_unchanged
exit_criteria:
  - cargo test -p ruscadb --test colbert_rerank -- test_ac_0072
  - cargo test -p ruscadb --test hybrid_retrieval --test cache_coherence --test robustness_misc
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - restaurar el proxy de overlap (overlap_terms) y eliminar el modulo de token-embeddings
sandbox:
  - cargo test -p ruscadb --test colbert_rerank
---

# SPEC-0072 — Reranker real: late-interaction (ColBERT-style) con hash embeddings

## Contexto

Línea futura «reranker cross-encoder real». ADR-016 excluye Candle/ONNX, así que
un cross-encoder entrenado no es viable **hoy**; la alternativa de frontera sin
runtime externo es el **late-interaction multi-vector** (ColBERT/ColBERTv2,
Khattab & Zaharia 2020) con **hash embeddings** deterministas (Svenstrup et al.
2017) para proyectar tokens a vectores densos sin modelo entrenado. Sustituye
el proxy de overlap de SPEC-0063 por MaxSim real, con la misma API pública.

## Superficie (interna)

- `TOKEN_DIM = 32` (const con nombre).
- `token_embedding(token: &str) -> [f32; TOKEN_DIM]`: hash FNV-1a + splitmix64
  → componentes en `[-1, 1]` normalizados (determinista, sin estado).
- `maxsim(query_tokens: &[String], doc_tokens: &[String]) -> f32`:
  `Σ_i max_j cos(q_i, d_j)`; 0 si algún lado es vacío.
- `rescore_or_keep` re-puntúa con MaxSim (los scores dejan de ser enteros).

## Criterios de aceptación

- **AC-0072-01/02** — MaxSim premia cobertura e ignora repetición.
- **AC-0072-03** — el híbrido usa MaxSim (stage y scores continuos).
- **AC-0072-04/05** — determinismo y degradación intacta.

## Trazabilidad

Tests `test_ac_0072_<nn>_*`; verificado por `cargo xtask trace`/`contract`.