---
id: SPEC-0110
feature: columnar_engine
status: proposed
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/columnar.rs]
  out_of_scope: [distributed, full SQL OLAP, parquet export]
fr:
  - { id: FR-0110-01, desc: "feature flag `columnar` (off por defecto) habilita un engine columnar opcional" }
  - { id: FR-0110-02, desc: "ColumnarTable soporta append de filas y scan columnar basico" }
nf:
  - { id: NF-0110-01, desc: "cero dependencias nuevas en el build por defecto; Arrow opcional detras del flag" }
acceptance_criteria:
  - id: AC-0110-01
    given: la feature `columnar` activada
    when: se crea una ColumnarTable y se anaden filas
    then: el scan devuelve las filas en orden
    test: test_ac_0110_01_columnar_append_scan
exit_criteria:
  - cargo test -p ruscadb --all-features --test columnar
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0110 — Engine columnar opcional (Arrow/Lance)

## Contexto

Frontier 2026 (DuckDB, LanceDB): almacenamiento columnar para analytics y
datasets grandes de vectores. RuscaDB es row-oriented (heap paginado). Esta spec
añade un engine columnar opcional detrás de un feature flag, sin cambiar el
runtime por defecto (patrón SPEC-0060/ADR-013).

## Superficie

- Feature `columnar` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/columnar.rs` — `ColumnarTable` con append + scan.
- Integración futura: parquet export, vector columnar, hybrid search.

## Notas

- Implementación inicial: columnar en memoria (Vec por columna), sin Arrow.
- Arrow/Lance se añadirán cuando se requiera interoperabilidad con DuckDB/LanceDB.
---
