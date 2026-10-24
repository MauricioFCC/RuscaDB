---
id: SPEC-0124
feature: arrow_columnar
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/arrow_columnar.rs]
  out_of_scope: [parquet export, distributed, full SQL OLAP]
fr:
  - { id: FR-0124-01, desc: "feature flag `arrow-columnar` (off por defecto) habilita engine columnar Arrow" }
  - { id: FR-0124-02, desc: "ArrowColumnarTable soporta append de filas y scan columnar" }
nf:
  - { id: NF-0124-01, desc: "arrow crate opcional detras del flag; build por defecto sin deps nuevas" }
acceptance_criteria:
  - id: AC-0124-01
    given: la feature `arrow-columnar` activada
    when: se crea una ArrowColumnarTable y se anaden filas
    then: el scan devuelve las filas en orden
    test: test_ac_0124_01_arrow_append_scan
exit_criteria:
  - cargo test -p ruscadb --all-features --test arrow_columnar
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0124 — Engine columnar Arrow (opcional)

## Contexto

Frontier 2026 (LLKV, LanceDB, DuckDB): almacenamiento columnar Arrow para
zero-copy reads y predicate pushdown. RuscaDB tiene columnar en memoria (SPEC-0110).
Esta spec añade Arrow real detrás de un feature flag.

## Superficie

- Feature `arrow-columnar` en `ruscadb` (off por defecto).
- Dependencia opcional `arrow = "60"`.
- `crates/ruscadb/src/arrow_columnar.rs` — `ArrowColumnarTable` con append + scan.

## Notas

- Implementación inicial: Arrow arrays (Int64, Float64, Utf8) con append + scan.
- Parquet export y predicate pushdown quedan como trabajo futuro.
---
