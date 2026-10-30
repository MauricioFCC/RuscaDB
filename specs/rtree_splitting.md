---
id: SPEC-0156
feature: rtree_splitting
status: implemented
maturity: real
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/rtree_split.rs]
  out_of_scope: [R*-tree, Hilbert R-tree, KNN espacial]
fr:
  - { id: FR-0156-01, desc: "feature flag `rtree-split` (off por defecto) habilita R-tree con splitting" }
  - { id: FR-0156-02, desc: "RTreeSplit::insert inserta con splitting heuristico" }
nf:
  - { id: NF-0156-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0156-01
    given: la feature `rtree-split` activada
    when: se insertan puntos y se consulta por bounding box
    then: los puntos dentro del box se devuelven
    test: test_ac_0156_01_rtree_split_range
exit_criteria:
  - cargo test -p ruscadb --all-features --test rtree_split
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0156 â€” R-tree con splitting heurÃ­stico

## Contexto

Frontier 2026 (PostGIS, R*-tree): R-tree con splitting heurÃ­stico para
bÃºsqueda espacial eficiente. RuscaDB tiene R-tree lineal (SPEC-0114).
Esta spec aÃ±ade splitting detrÃ¡s de un feature flag.

## Superficie

- Feature `rtree-split` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/rtree_split.rs` â€” `RTreeSplit` con `insert` + `range_query`.
- IntegraciÃ³n futura: R*-tree, Hilbert R-tree, KNN espacial.

## Estado del arte

- **R*-tree**: splitting heurÃ­stico con reinsertion.
- **Hilbert R-tree**: ordenamiento por curva de Hilbert.
- **RuscaDB actual**: R-tree lineal (SPEC-0114, sin splitting).

## DiseÃ±o propuesto

1. **RTreeSplit**: R-tree con splitting lineal (Guttman).
2. **Splitting**: cuando un nodo excede la capacidad, se divide.
3. **Range query**: bÃºsqueda por bounding box.

## Notas

- ImplementaciÃ³n inicial: splitting lineal simplificado.
- R*-tree y Hilbert R-tree quedan como trabajo futuro.
---
