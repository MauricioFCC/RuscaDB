---
id: SPEC-0104
feature: multi_index
status: implemented
owner: query-team
appetite_days: 3
boundaries:
  crates: [ruscadb]
  files: [crates/ruscadb/src/catalog.rs, crates/ruscadb/src/index.rs, crates/ruscadb/src/table_api.rs, crates/ruscadb/src/executor.rs, crates/ruscadb/src/join.rs, crates/ruscadb/src/dml.rs, crates/ruscadb/src/delete.rs]
  out_of_scope: [índices compuestos (multi-columna), DROP INDEX, índices únicos/constraints, orden por índice (ORDER BY), índices sobre más de una tabla]
fr:
  - { id: FR-0104-01, desc: "TableDef guarda una lista de índices (uno por columna) y el catálogo la persiste en formato v3/v4 con lectura retrocompatible v1/v2" }
  - { id: FR-0104-02, desc: "create_index admite varias columnas por tabla y rechaza duplicar la misma; el planner usa el índice de la columna del WHERE" }
  - { id: FR-0104-03, desc: "INSERT/UPDATE/DELETE mantienen todos los índices (entrada por cada columna indexada, sin huérfanos)" }
nf:
  - { id: NF-0104-01, desc: "una base persistida con el formato de un índice (v1/v2) carga sin migración; una versión desconocida es CorruptManifest accionable" }
  - { id: NF-0104-02, desc: "sin índices el plan es FullScan; con índice en la columna del filtro es IndexScan (cero regresión del planner)" }
acceptance_criteria:
  - id: AC-0104-01
    given: una tabla con índices en `a`, `b` y `c`
    when: se consulta `WHERE` sobre cada columna y se inspecciona con `EXPLAIN`
    then: cada filtro usa `IndexScan` sobre su columna y devuelve la fila correcta
    test: test_ac_0104_01_multiple_indexes_are_used_per_column
  - id: AC-0104-02
    given: una tabla con índices en `a` y `c`
    when: se ejecutan INSERT, UPDATE (cambia ambas columnas) y DELETE
    then: ambos índices quedan consistentes (valores viejos fuera, nuevos dentro)
    test: test_ac_0104_02_dml_keeps_every_index_consistent
  - id: AC-0104-03
    given: una tabla con un índice en `a`
    when: se crea otro índice sobre `a` y luego sobre otra columna
    then: el duplicado falla con mensaje que nombra la columna; el segundo índice se crea
    test: test_ac_0104_03_duplicate_column_index_is_actionable
  - id: AC-0104-04
    given: una tabla con dos índices persistida
    when: se cierra y reabre la base
    then: el catálogo recarga ambos índices y siguen respondiendo
    test: test_ac_0104_04_multiple_indexes_survive_reopen
  - id: AC-0104-05
    given: un catálogo con dos índices en memoria
    when: se codifica y decodifica
    then: el roundtrip multi-índice es exacto (columnas, páginas y consultas por columna)
    test: test_ac_0104_05_catalog_roundtrip_multi_index
  - id: AC-0104-06
    given: un payload legacy (v1/v2) con el marcador `0` (sin índice)
    when: se decodifica con el lector legacy
    then: la tabla carga sin índices (compatibilidad de lectura)
    test: test_ac_0104_06_legacy_payload_without_index_loads
  - id: AC-0104-07
    given: un payload legacy (v1/v2) con el marcador `1` (su único índice)
    when: se decodifica con el lector legacy
    then: la tabla carga con exactamente ese índice (compatibilidad de lectura)
    test: test_ac_0104_07_legacy_payload_with_single_index_loads
  - id: AC-0104-08
    given: un payload legacy con un marcador de índice inválido
    when: se decodifica
    then: falla con un error accionable que nombra la marca inválida
    test: test_ac_0104_08_legacy_invalid_index_marker_is_actionable
exit_criteria:
  - cargo test -p ruscadb --test multi_index
  - cargo test -p ruscadb --all-features
  - cargo xtask trace / contract verdes
rollback:
  - revertir el commit de SPEC-0104 (el formato v1/v2 de lectura sigue soportado)
sandbox:
  - cargo test -p ruscadb --test multi_index
---

# SPEC-0104 — Varios índices secundarios por tabla

## Contexto

Límite del MVP: «un índice secundario por tabla (una columna)». Esta spec
permite **varios índices** por tabla (uno por columna), sobre la misma
infraestructura de array ordenado por página (SPEC-0012).

## Superficie

- `catalog.rs`: `TableDef.indexes: Vec<IndexDef>`, códec del catálogo en
  versiones **v3** (reservado) y **v4** (paginado con overflow), con lectura
  retrocompatible de **v1/v2** (marcador `u8` de un índice).
- `index.rs`: `index_insert`/`index_remove`/`index_lookup_eq`/`index_build`
  reciben la columna; `indexed_column` devuelve tipo y páginas del índice de
  esa columna.
- `table_api.rs` / `delete.rs` / `dml.rs`: el mantenimiento itera **todos** los
  índices de la tabla.
- `executor.rs` / `join.rs`: el planner elige el índice de la columna del
  `WHERE`/`ON`.

## Criterios de aceptación

- **AC-0104-01** — planner e índice por columna.
- **AC-0104-02** — mantenimiento DML en todos los índices.
- **AC-0104-03** — duplicado ruidoso; segundo índice permitido.
- **AC-0104-04** — persistencia del catálogo multi-índice.
- **AC-0104-05..08** — códec, compatibilidad legacy y error accionable.

## Trazabilidad

Tests `test_ac_0104_<nn>_*` con marcador `@spec AC-0104-NN`; verificado por
`cargo xtask trace`/`contract`.
