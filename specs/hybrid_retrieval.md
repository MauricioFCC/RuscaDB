---
id: SPEC-0063
feature: hybrid_retrieval
status: implemented
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-query, ruscadb-fts]
  out_of_scope: [path tensorial completo ColBERT, cross-encoder en hot path, SPLADE, reescritura de BM25]
fr:
  - { id: FR-0063-01, desc: "fusion RRF (rank inversion, k_reciprocal=60) sobre el top-N_texto (BM25) y el top-N_vec (KNN), con N>k; devuelve los k mejores por score fusionado (score-agnostic, sin mezclar magnitudes)" }
  - { id: FR-0063-02, desc: "re-scoring late-interaction MaxSim sobre el top-k_rerank de la lista RRF (patron TRF, arXiv:2508.01405), opcional y determinista; si no hay embeddings palabra-nivel queda no-op y se sirve la lista RRF" }
  - { id: FR-0063-03, desc: "degradacion graceful por ausencia de una via: solo-texto sin vector => BM25 puro; solo-vector sin texto => KNN puro; sin coincidencias de termino => orden identico al KNN puro" }
nf:
  - { id: NF-0063-01, desc: "cero regresion: sin coincidencias de termino el resultado es identico al KNN puro; la API se anade y NO altera execute/execute_at ni la firma publica existente" }
  - { id: NF-0063-02, desc: "sin red: el nucleo RRF + MaxSim es local y offline; el reranker cross-encoder (MiniLM/FlashRank) vive detras de una feature `onnx` opcional, fuera del hot path" }
  - { id: NF-0063-03, desc: "API aditiva y compatible: cero breaking changes; errores accionables WHAT+WHY+WHERE; docstrings ES; sin magic numbers (constantes con nombre); sin unsafe" }
acceptance_criteria:
  - id: AC-0063-01
    given: una tabla con FTS y HNSW donde un subconjunto de ids aparece en ambas listas top-N
    when: se ejecuta hybrid_search con text_query y vector_query
    then: la interseccion queda priorizada (mas alto en la fusion RRF) que los hits presentes en una sola via
    test: test_ac_0063_01_rrf_prioritizes_overlap
  - id: AC-0063-02
    given: una tabla solo-FTS (sin indice vectorial) y una consulta hibrida con vector_query no vacio
    when: se ejecuta hybrid_search
    then: degrada a BM25 puro, sin error, con el mismo orden que text_search sobre la columna
    test: test_ac_0063_02_text_only_degrades_to_bm25
  - id: AC-0063-03
    given: una tabla con HNSW pero sin columna TEXT indexada y una consulta hibrida con text_query no vacio
    when: se ejecuta hybrid_search
    then: degrada a KNN puro, sin error, con el mismo orden que el KNN clasico sobre el vector
    test: test_ac_0063_03_vector_only_degrades_to_knn
  - id: AC-0063-04
    given: un top-k_rerank de RRF con un oraculo manual determinista de relevancia latente (token overlap)
    when: se aplica el re-scoring MaxSim
    then: el orden final sigue la relevancia latente del oraculo (reordena candidatos respecto a RRF) y es reproducible
    test: test_ac_0063_04_maxsim_rescore_reorders_candidates
  - id: AC-0063-05
    given: la suite de queries RQL existente (SELECT con MATCH y KNN) sobre una base poblada
    when: se compila y ejecuta con la API hibrida presente
    then: execute/execute_at devuelve exactamente los mismos resultados que antes (API aditiva, sin regresion)
    test: test_ac_0063_05_api_is_additive_no_execute_regression
exit_criteria:
  - cargo test -p ruscadb -p ruscadb-query -p ruscadb-fts -- test_ac_0063
  - cargo test -p ruscadb -p ruscadb-query -p ruscadb-fts  (suite previa verde)
  - cargo clippy -p ruscadb -p ruscadb-query -p ruscadb-fts --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, mutation score acotado a los ficheros tocados >= 70%
  - cargo xtask trace  (cada AC tiene test test_ac_0063_<nn>_*)
rollback:
  - eliminar el metodo Database::hybrid_search y el modulo de fusion/RRF/MaxSim; el
    executor KNN, el indexado FTS y Database::execute quedan exactamente como estaban
    (la feature es un modulo adicional, sin tocar el hot path existente)
sandbox:
  - cargo test -p ruscadb --test hybrid_retrieval
---

# SPEC-0063 — Búsqueda híbrida (RRF + re-scoring MaxSim)

## Contexto

**Revalidación de frontera §3 exige un recuperador híbrido término+vector.** Hoy
conviven dos vías independientes en el motor: FTS BM25 (`ruscadb-fts`, índice
invertido expuesto por `TableIndexes::text_search`) y KNN vectorial
(`ruscadb-vector` HNSW, servido por `TableIndexes::filtered_vector_search` /
`ifvs_vector_search` e `apply_knn` en el executor). No existe una consulta que
combine ambas señales.

La frontera 2026 converge en un patrón de tres etapas (recall → fusión →
re-score): **RRF como base score-agnostic** (robusta ante magnitudes BM25 vs
distancia incompatibles), **re-scoring late-interaction MaxSim** sobre el
top-N de RRF (patrón TRF, arXiv:2508.01405, que reporta ~+8.1 % nDCG@10 sobre
RRF puro) y un **reranker cross-encoder pequeño opcional** (MiniLM/FlashRank)
detrás de la feature `onnx` y fuera del hot path. El núcleo (RRF + MaxSim) es
**local, determinista y sin red**.

## Superficie de API

API principal (aditiva, no rompe `execute`):

```rust
// crates/ruscadb
impl Database {
    /// Búsqueda híbrida término+vector. Ver `specs/hybrid_retrieval.md` (SPEC-0063).
    pub fn hybrid_search(
        &mut self,
        table: &str,
        text_query: Option<&str>,     // columna TEXT + términos [asumir: columna explicita]
        vector_query: Option<&[f32]>, // vector de consulta para el HNSW
        k: usize,
        weights: HybridWeights,
    ) -> Result<Vec<HybridHit>, RuscaError>;
}

pub struct HybridWeights { pub text: f32, pub vector: f32 } // por defecto iguales

pub struct HybridHit {
    pub id: RecordId,
    pub score: f32, // score de la etapa final (RRF o MaxSim si re-scoreo)
    pub stage: HybridStage, // Rrf | MaxSim | Bm25Only | KnnOnly
}
```

Constantes con nombre (sin magic numbers): `RRF_K_RECIPROCAL = 60`,
`RRF_CANDIDATES_MULTIPLIER` (N = `k * MULTIPLIER`, N > k),
`DEFAULT_RERANK_DEPTH` (top-k_rerank re-evaluado por MaxSim).

`[asumir]` piezas que deben existir y no están hoy:

- `Database::text_search(table, column, query, k)` **público**: hoy solo existe
  `TableIndexes::text_search` como `pub(crate)` en `crates/ruscadb/src/indexes.rs:272`.
  Alternativa sin nueva API: reusar `Database::execute` con un `SELECT ... MATCH`.
- Acceso interno del híbrido a `TableIndexes` (`filtered_vector_search` /
  `ifvs_vector_search`) para leer los `RecordId` del top-N_vec sin pasar por RQL.
- Almacén de **embeddings palabra-nivel** por registro (multi-vector) para
  MaxSim real; hoy solo existe embedding por registro (`record.vector`).

Cláusula RQL opcional (fase posterior, `[asumir]` lexer/parser): `HYBRID`
como azúcar sobre `Database::hybrid_search`, sin tocar el AST `Select` en esta
iteración (`crates/ruscadb-query/src/ast.rs:248`).

## Diseño (pipeline)

1. **Recall término**: top-N_texto por BM25 vía `TableIndexes::text_search`
   (solo si hay columna TEXT indexada y `text_query` presente).
2. **Recall vector**: top-N_vec por KNN vía `filtered_vector_search` (sin
   `WHERE`) o `ifvs_vector_search` (con filtro) sobre el `HnswIndex` real
   (`crates/ruscadb/src/indexes.rs`), con N > k.
3. **Fusión RRF**: `score(d) = Σ_L w_L / (RRF_K_RECIPROCAL + rank_L(d))`,
   `rank` base 1 por lista; desempate determinista por `RecordId`.
4. **Re-score MaxSim** (opcional): sobre el top-k_rerank de la fusión, late
   interaction término-vector (max sobre similitudes de token embeddings).
   Si no hay embeddings palabra-nivel, es no-op y se sirve la lista RRF.
5. **Reranker cross-encoder** (feature `onnx`, fuera del hot path): reordena el
   top-k_rerank ya fusionado; **deshabilitado por defecto**.
6. Devuelve los `k` mejores como `HybridHit`.

**Degradación**: lista RRF con una sola vía ⇒ orden idéntico a esa vía
(BM25 o KNN puro). Sin coincidencias de término queda solo la lista vectorial ⇒
identidad con KNN puro (NF-0063-01).

## Fuentes de frontera

- RRF (Cormack et al.): fusión por inversión de rango, score-agnostic.
- TRF late-interaction, arXiv:2508.01405: MaxSim sobre top-N de RRF (+~8.1 % nDCG@10).
- ColBERT (MaxSim): referencia del late interaction; el path tensorial completo
  queda fuera de alcance.

## Criterios de aceptación

- **AC-0063-01** — fusión RRF prioriza la intersección texto∩vector.
- **AC-0063-02/03** — degradación a BM25 puro / KNN puro sin error.
- **AC-0063-04** — MaxSim reordena según oráculo determinista de relevancia latente.
- **AC-0063-05** — API aditiva: `execute` sin regresión.

## Trazabilidad

Tests `test_ac_0063_<nn>_*`; verificado por `cargo xtask trace`.

## Decisiones abiertas

1. **¿Dónde vive la fusión?** En el executor (`crates/ruscadb/src/executor.rs`,
   junto a `apply_knn`) para compartir el pipeline de `Select`, o en la fachada
   (`Database::hybrid_search`) como composition root y dejar el executor intacto
   en esta iteración.
2. **¿Cómo se pasan los `weights`?** Struct `HybridWeights { text, vector }` por
   valor, o `(f32, f32)` posicional, o recíprocos normalizados; y si los pesos
   también deben aceptarse desde una futura cláusula RQL `HYBRID`.
3. **¿MaxSim requiere embeddings palabra-nivel nuevos?** ¿Se introduce un almacén
   multi-vector por token (nuevo `[asumir]`) o se define un proxy determinista de
   overlap término-vector en el MVP y el path tensorial real queda para una
   spec posterior?
