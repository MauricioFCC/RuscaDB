---
id: SPEC-0152
feature: fts_proximity
status: proposed
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-fts]
  files: [crates/ruscadb/src/fts_proximity.rs]
  out_of_scope: [modelos de lenguaje, embeddings semánticos]
fr:
  - { id: FR-0152-01, desc: "feature flag `fts-proximity` (off por defecto) habilita busqueda por proximidad" }
  - { id: FR-0152-02, desc: "ProximityMatcher::match_proximity verifica distancia entre terminos" }
nf:
  - { id: NF-0152-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0152-01
    given: la feature `fts-proximity` activada
    when: se busca una frase con distancia maxima
    then: se devuelve true si los terminos estan dentro de la distancia
    test: test_ac_0152_01_proximity_match
exit_criteria:
  - cargo test -p ruscadb --all-features --test fts_proximity
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0152 — Búsqueda por proximidad (phrase queries con distancia)

## Contexto

Frontier 2026 (Elasticsearch, Lucene): búsqueda por proximidad (slop).
RuscaDB tiene phrase matching exacto (SPEC-0117). Esta spec añade proximidad
detrás de un feature flag.

## Superficie

- Feature `fts-proximity` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/fts_proximity.rs` — `ProximityMatcher` con `match_proximity`.
- Integración futura: RQL `MATCH ... SLOP n`.

## Estado del arte

- **Lucene**: phrase queries con slop (distancia máxima entre términos).
- **Elasticsearch**: match_phrase con slop.
- **RuscaDB actual**: phrase matching exacto (SPEC-0117).

## Diseño propuesto

1. **ProximityMatcher**: verifica si los términos de una frase aparecen dentro
   de una distancia máxima (slop).
2. **Slop**: número máximo de posiciones entre términos consecutivos.
3. **Scoring**: score basado en la distancia real (menor distancia = mayor score).

## Notas

- Implementación inicial: slop básico (distancia entre términos consecutivos).
- Scoring avanzado queda como trabajo futuro.
---
