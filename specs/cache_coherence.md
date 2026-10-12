---
id: SPEC-0069
feature: cache_coherence
status: implemented
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb]
  out_of_scope: [persistencia de la cache, invalidacion selectiva por tabla, TTL]
fr:
  - { id: FR-0069-01, desc: "toda mutacion (insert_record, insert_many, update, delete) invalida las ENTRADAS del cache semantico (conservando hits/misses acumulados)" }
  - { id: FR-0069-02, desc: "la clave de cache es la FIRMA de la query (texto con el vector enmascarado: tabla, k, WHERE, LIMIT) + el vector; un hit exige firma identica y cos >= 0.8" }
  - { id: FR-0069-03, desc: "cosine con dimensiones distintas devuelve 0.0 (nunca hit); cache_store solo reemplaza filas de la MISMA firma" }
nf:
  - { id: NF-0069-01, desc: "cero regresion en SPEC-0065 (OFF por defecto, hit/miss/clear y coste) — la invalidacion es adicional" }
  - { id: NF-0069-02, desc: "sin panics; sin dependencias nuevas; errores accionables WHAT+WHY+WHERE" }
acceptance_criteria:
  - id: AC-0069-01
    given: cache ON con una KNN ya cacheada sobre una tabla
    when: se inserta una fila nueva y se repite la KNN con un vector similar (cos >= 0.8)
    then: es MISS (no hit) y el resultado incluye la fila nueva (sin datos obsoletos)
    test: test_ac_0069_01_write_invalidates_cache
  - id: AC-0069-02
    given: cache ON con una KNN cacheada sobre la tabla A
    when: se ejecuta la misma KNN sobre la tabla B (mismo vector y k)
    then: es MISS (la firma incluye la tabla) y devuelve las filas de B
    test: test_ac_0069_02_signature_includes_table
  - id: AC-0069-03
    given: cache ON con una KNN k=1 cacheada con el vector v
    when: se ejecuta la KNN con k=2 y un vector similar a v
    then: es MISS (la firma incluye k) y devuelve 2 filas
    test: test_ac_0069_03_signature_includes_k
  - id: AC-0069-04
    given: una clave cacheada de dimension 3
    when: se consulta con un vector de dimension 2 (o viceversa)
    then: cosine devuelve 0.0, no hay hit y se ejecuta el motor
    test: test_ac_0069_04_dimension_mismatch_never_hits
  - id: AC-0069-05
    given: cache ON con hits/misses acumulados
    when: se ejecuta una mutacion
    then: las entradas quedan a cero pero hits/misses se conservan; la query siguiente es miss
    test: test_ac_0069_05_invalidation_keeps_metrics
exit_criteria:
  - cargo test -p ruscadb --test cache_coherence -- test_ac_0069
  - cargo test -p ruscadb --test semantic_cache  (SPEC-0065 sigue verde)
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - revertir el modulo cache al estado SPEC-0065 (sin invalidacion ni firma)
sandbox:
  - cargo test -p ruscadb --test cache_coherence
---

# SPEC-0069 — Coherencia del caché semántico (retrospectiva 3)

## Contexto

**Hallazgos P1 de la retrospectiva 3** sobre SPEC-0065: (1) el caché nunca se
invalida al escribir → sirve filas obsoletas/borradas; (2) la clave ignora
tabla, `k`, `WHERE` y `LIMIT` → falsos hits entre consultas distintas; (3)
`cosine` trunca dimensiones con `zip` → vectores de distinta dim se consideran
idénticos; (4) `cache_store` reemplaza filas conservando el vector clave viejo.
Esta spec corrige los cuatro con firma de query + invalidación en mutaciones.

## Superficie de API (interna)

- `CacheEntry { signature: String, vector: Vec<f32>, rows: Vec<Row> }`.
- `query_signature(query) -> Option<String>`: texto con el bloque `[...]` del
  vector enmascarado (`?`), normalizado (espacios colapsados).
- `invalidate_semantic_cache()` (pub(crate)): vacía entradas, conserva métricas.
- `cosine` devuelve `0.0` si `a.len() != b.len()`.
- Hooks de invalidación en `insert_record`, `insert_many`, `execute_update`,
  `execute_delete` y `delete`.

## Criterios de aceptación

- **AC-0069-01/05** — invalidación por escritura (y métricas conservadas).
- **AC-0069-02/03** — firma con tabla y `k`.
- **AC-0069-04** — dimensiones distintas nunca hacen hit.

## Trazabilidad

Tests `test_ac_0069_<nn>_*`; verificado por `cargo xtask trace`/`contract`.