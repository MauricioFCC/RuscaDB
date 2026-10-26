---
id: SPEC-0159
feature: query_result_cache
status: implemented
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/query_cache.rs]
  out_of_scope: [distributed caching, cache invalidation policies]
fr:
  - { id: FR-0159-01, desc: "feature flag `query-cache` (off por defecto) habilita cache de resultados" }
  - { id: FR-0159-02, desc: "QueryCache::get/set almacena y recupera resultados" }
nf:
  - { id: NF-0159-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0159-01
    given: la feature `query-cache` activada
    when: se almacena y recupera un resultado
    then: el resultado es identico
    test: test_ac_0159_01_query_cache
exit_criteria:
  - cargo test -p ruscadb --all-features --test query_cache
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0159 — Caché de resultados de consultas

## Contexto

Frontier 2026 (DuckDB, PostgreSQL): materialización de resultados de consultas
costosas. RuscaDB tiene caché semántico (SPEC-0065/0069). Esta spec añade
caché de resultados básico detrás de un feature flag.

## Superficie

- Feature `query-cache` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/query_cache.rs` — `QueryCache` con `get`/`set`.
- Integración futura: invalidación por mutación, TTL.

## Estado del arte

- **DuckDB**: result cache para consultas repetidas.
- **PostgreSQL**: materialized views.
- **RuscaDB actual**: caché semántico (SPEC-0065/0069).

## Diseño propuesto

1. **QueryCache**: almacena resultados de consultas por firma.
2. **get/set**: recuperar/almacenar resultados.
3. **Invalidación**: por mutación o TTL.

## Notas

- Implementación inicial: caché en memoria (sin persistencia).
- Persistencia y TTL quedan como trabajo futuro.
---
