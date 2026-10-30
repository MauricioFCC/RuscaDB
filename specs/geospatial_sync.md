---
id: SPEC-0112
feature: geospatial_sync
status: implemented
maturity: stub
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/geo.rs]
  out_of_scope: [clustering, replicacion multi-nodo, protocolo de sync distribuido]
fr:
  - { id: FR-0112-01, desc: "feature flag `geo` (off por defecto) habilita tipos geoespaciales basicos" }
  - { id: FR-0112-02, desc: "Point y distancia euclidiana helper" }
nf:
  - { id: NF-0112-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0112-01
    given: la feature `geo` activada
    when: se crea un Point y se calcula distancia
    then: la distancia es correcta
    test: test_ac_0112_01_geo_distance
exit_criteria:
  - cargo test -p ruscadb --all-features --test geo
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0112 â€” Geoespacial + sync (stub)

## Contexto

Frontier 2026 (SurrealDB): geospatial + sync protocol para edge. RuscaDB no
tiene geospatial ni sync distribuido. Esta spec aÃ±ade tipos geoespaciales
bÃ¡sicos detrÃ¡s de un feature flag, sin dependencias externas.

## Superficie

- Feature `geo` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/geo.rs` â€” `Point { x, y }` + `distance`.
- Sync protocol: fuera de alcance (futuro, requiere diseÃ±o de red).

## Notas

- ImplementaciÃ³n inicial: geometrÃ­a 2D euclidiana, sin proyecciones ni Ã­ndices espaciales.
- Ãndices espaciales (R-tree) y sync protocol quedan como trabajo futuro.
---
