---
id: SPEC-0065
feature: semantic_cache
status: implemented
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb]
  out_of_scope: [cache persistente en disco, cache de queries SQL arbitrarias, TTL]
fr:
  - { id: FR-0065-01, desc: "cache semantico de KNN opcional: clave = embedding de consulta; hit si cos >= 0.8 con una clave previa; devuelve el resultado cacheado" }
  - { id: FR-0065-02, desc: "coste por query: CostReport con queries, rows_returned y nanos acumulados por execute/execute_at, reseteable" }
nf:
  - { id: NF-0065-01, desc: "cero regresion: cache OFF por defecto; ON solo para queries KNN y el resultado servido es identico al del motor" }
  - { id: NF-0065-02, desc: "sin red; sin dependencias nuevas; errores accionables WHAT+WHY+WHERE" }
acceptance_criteria:
  - id: AC-0065-01
    given: cache semantico habilitado y una consulta KNN ejecutada con vector v
    when: se ejecuta una consulta KNN con vector v' tal que cos(v,v') >= 0.8 (mismo top-k esperado)
    then: hit (hits == 1), resultados identicos a la primera consulta, misses == 0
    test: test_ac_0065_01_knn_semantic_hit
  - id: AC-0065-02
    given: cache OFF (por defecto) y dos consultas KNN identicas
    when: se ejecutan
    then: hits == 0 y los resultados son correctos (sin reutilizacion)
    test: test_ac_0065_02_cache_off_by_default
  - id: AC-0065-03
    given: cache habilitado con hits acumulados
    when: se ejecuta clear_semantic_cache
    then: metrics vuelven a cero de hits/misses y una consulta nueva es miss
    test: test_ac_0065_03_cache_clear_resets_metrics
  - id: AC-0065-04
    given: N consultas ejecutadas (mezcla SELECT y KNN)
    when: se pide cost_report()
    then: queries == N, rows_returned == suma de filas y nanos > 0; reset_cost_report deja todo a cero
    test: test_ac_0065_04_cost_report_accumulates_and_resets
  - id: AC-0065-05
    given: cache habilitado con una clave almacenada
    when: se ejecuta KNN con vector ortogonal (cos < 0.8)
    then: miss (misses == 1), resultado correcto del motor y sin reutilizar
    test: test_ac_0065_05_distinct_vector_misses
exit_criteria:
  - cargo test -p ruscadb --test semantic_cache -- test_ac_0065
  - cargo test -p ruscadb  (suite previa verde)
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - eliminar el modulo semantic_cache; execute vuelve al camino directo (sin Instant ni cache)
sandbox:
  - cargo test -p ruscadb --test semantic_cache
---

# SPEC-0065 — Caché semántico + coste por query

## Contexto

Línea futura del roadmap: **caché semántico + coste por query**. La
investigación de frontera (Gupta et al. 2024, GPT Semantic Cache) indica que
cachear por embedding de consulta con threshold de coseno ~0.8 reduce las
llamadas hasta ~68.8 %. En ruscaDB embebido esto es nativo: el KNN ya recibe un
vector; el caché reutiliza consultas semánticamente iguales sin reindexar. El
coste por query añade contadores de la fachada (queries, filas, tiempo) — sin
tocar el executor.

## Superficie de API

```rust
impl Database {
    pub fn set_semantic_cache(&mut self, enabled: bool);
    pub fn semantic_cache_metrics(&self) -> CacheMetrics; // { hits, misses, entries }
    pub fn clear_semantic_cache(&mut self);
    pub fn cost_report(&self) -> CostReport;              // { queries, rows_returned, nanos }
    pub fn reset_cost_report(&mut self);
}
```

Caché: en `execute`/`execute_at`, si la query es KNN (`SELECT ... KNN`), se
extrae el vector (parsing del texto entre `[ ]`), se busca por coseno ≥ umbral
(`SEMANTIC_CACHE_THRESHOLD = 0.8`) entre claves; hit → devuelve el `Vec<Row>`
almacenado; miss → ejecuta, almacena `(vector, rows)` y devuelve. Métrica por
`Instant` (ya usado en el executor) acumulada en `CostReport`.

## Criterios de aceptación

- **AC-0065-01/03/05** — hits, clear y misses del caché semántico.
- **AC-0065-02** — OFF por defecto (cero regresión).
- **AC-0065-04** — coste por query acumulativo y reseteable.

## Trazabilidad

Tests `test_ac_0065_<nn>_*`; verificado por `cargo xtask trace`/`contract`.