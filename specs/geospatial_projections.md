---
id: SPEC-0140
feature: geospatial_projections
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/geo_proj.rs]
  out_of_scope: [tiles, rendering, map projections beyond WGS84]
fr:
  - { id: FR-0140-01, desc: "feature flag `geo-proj` (off por defecto) habilita proyecciones geograficas" }
  - { id: FR-0140-02, desc: "GeoPoint con lat/lon y conversion a coordenadas cartesianas" }
nf:
  - { id: NF-0140-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0140-01
    given: la feature `geo-proj` activada
    when: se crea un GeoPoint y se calcula distancia Haversine
    then: la distancia es correcta
    test: test_ac_0140_01_haversine_distance
exit_criteria:
  - cargo test -p ruscadb --all-features --test geo_proj
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0140 — Proyecciones geográficas (WGS84)

## Contexto

Frontier 2026 (SurrealDB, PostGIS): geospatial con proyecciones y distancias
reales. RuscaDB tiene Point/Rect (SPEC-0112). Esta spec añade proyecciones
geográficas detrás de un feature flag.

## Superficie

- Feature `geo-proj` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/geo_proj.rs` — `GeoPoint` con lat/lon + Haversine.
- Integración futura: tiles, rendering, índices espaciales.

## Estado del arte

- **PostGIS**: proyecciones SRID, distancias geodésicas.
- **SurrealDB**: geospatial nativo en SurrealQL.
- **RuscaDB actual**: Point/Rect euclidiano (SPEC-0112).

## Diseño propuesto

1. **GeoPoint**: lat/lon con validación.
2. **Haversine**: distancia entre dos puntos en la esfera.
3. **Conversión**: lat/lon → coordenadas cartesianas.

## Notas

- Implementación inicial: Haversine sin proyecciones complejas.
- Proyecciones SRID y tiles quedan como trabajo futuro.
---
