---
id: SPEC-0028
feature: facade_ergonomics_gc
status: accepted
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-txn, ruscadb-core]
  out_of_scope: [backend alterno, lectores concurrentes, purga en grafo/fts]
fr:
  - { id: FR-0028-01, desc: "DatabaseBuilder fluido (data_path, pool_capacity, encryption) culminado en open()" }
  - { id: FR-0028-02, desc: "Database::tables() devuelve los nombres de tabla del catalogo" }
  - { id: FR-0028-03, desc: "Database::reap() ejecuta el GC MVCC sobre las filas obsoletas y devuelve el conteo" }
  - { id: FR-0028-04, desc: "el GC respeta el low_watermark del TxnManager (nunca purga versiones visibles)" }
nf:
  - { id: NF-0028-01, desc: "Builder sin estados invalidos: open falla si la config es invalida (mismo que DbConfig)" }
  - { id: NF-0028-02, desc: "reap es idempotente y no toca filas vivas ni recientes" }
acceptance_criteria:
  - id: AC-0028-01
    given: la API del builder
    when: se abre una base con Database::builder().data_path(..).pool_capacity(..).open()
    then: equivale a Database::open(DbConfig) y está lista para operar
    test: test_ac_0028_01_builder_opens_equivalent_database
  - id: AC-0028-02
    given: una base con tablas creadas
    when: se llama tables()
    then: devuelve los nombres exactos
    test: test_ac_0028_02_tables_lists_created_tables
  - id: AC-0028-03
    given: filas borradas por debajo del low_watermark
    when: se ejecuta reap()
    then: se eliminan fisicamente y el conteo devuelto coincide
    test: test_ac_0028_03_reap_purges_obsolete
  - id: AC-0028-04
    given: filas vivas y filas borradas por encima del watermark
    when: se ejecuta reap()
    then: ninguna se elimina (se conservan) y el conteo es 0
    test: test_ac_0028_04_reap_keeps_live_and_recent
  - id: AC-0028-05
    given: un builder con pool_capacity 0
    when: se llama open()
    then: devuelve el mismo error que DbConfig (InvalidConfig)
    test: test_ac_0028_05_builder_rejects_invalid_config
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0028
  - cargo mutants -p ruscadb mutation score >= 70%
rollback:
  - revertir la fachada al commit previo
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0028 — Ergonomía del composition root + GC cableado

## Contexto

Cierre de usabilidad del composition root (comparables: sled `Config`, surrealdb
builder, redb `Database::create`):

- **`DatabaseBuilder`**: constructor fluido en `crates/ruscadb` que culmina en
  `open()` y equivale a `Database::open(DbConfig)`.
- **`Database::tables()`**: introspección del catálogo (nombres de tabla).
- **`Database::reap()`**: cablea el reaper MVCC (`ruscadb-txn::gc`) a las filas
  físicas: escanea el heap, detecta versiones obsoletas por `low_watermark` y
  las purga, devolviendo el conteo. Nunca purga versiones visibles a snapshots
  futuros ni filas vivas. Documenta que no toca el grafo/FTS (fuera de alcance).

## Criterios de aceptación

- **AC-0028-01/02/05** — builder equivalente, introspección, config inválida.
- **AC-0028-03/04** — GC purga obsoletas y conserva vivas/recientes.

## Trazabilidad

Tests `test_ac_0028_<nn>_*` en `ruscadb`; verificado por `cargo xtask trace`.
