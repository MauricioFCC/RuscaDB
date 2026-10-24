---
id: SPEC-0114
feature: spatial_index
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/spatial.rs]
  out_of_scope: [proyecciones geograficas (WGS84), KNN espacial, tiles]
fr:
  - { id: FR-0114-01, desc: "feature flag `spatial` (off por defecto) habilita indices espaciales" }
  - { id: FR-0114-02, desc: "RTree soporta insert y range query (bounding box)" }
nf:
  - { id: NF-0114-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0114-01
    given: la feature `spatial` activada
    when: se inserta un punto y se consulta por bounding box
    then: el punto se devuelve si esta dentro del box
    test: test_ac_0114_01_rtree_range_query
exit_criteria:
  - cargo test -p ruscadb --all-features --test spatial
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0114 — Índice espacial (R-tree)

## Contexto

Frontier 2026 (SurrealDB, PostGIS): geospatial con índices espaciales.
RuscaDB tiene tipos Point/Rect (SPEC-0112) pero sin índice. Esta spec añade
un R-tree simple detrás de un feature flag.

## Superficie

- Feature `spatial` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/spatial.rs` — `RTree` con `insert` + `range_query`.

## Notas

- Implementación inicial: R-tree lineal (sin splitting heurístico).
- Proyecciones, KNN espacial y tiles quedan como trabajo futuro.
---
