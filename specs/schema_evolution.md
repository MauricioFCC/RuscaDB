---
id: SPEC-0160
feature: schema_evolution
status: proposed
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb/src/schema_evolution.rs]
  out_of_scope: [migraciones complejas, transformacion de datos]
fr:
  - { id: FR-0160-01, desc: "feature flag `schema-evolution` (off por defecto) habilita evolucion de esquema" }
  - { id: FR-0160-02, desc: "SchemaEvolution::add_column anade una columna a una tabla" }
nf:
  - { id: NF-0160-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0160-01
    given: la feature `schema-evolution` activada
    when: se anade una columna a una tabla
    then: la columna se anade correctamente
    test: test_ac_0160_01_add_column
exit_criteria:
  - cargo test -p ruscadb --all-features --test schema_evolution
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0160 — Evolución de esquema (add/drop columns)

## Contexto

Frontier 2026 (PostgreSQL, SQLite): evolución de esquema (ALTER TABLE).
RuscaDB tiene migraciones forward-only (SPEC-0077). Esta spec añade evolución
de esquema básica detrás de un feature flag.

## Superficie

- Feature `schema-evolution` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/schema_evolution.rs` — `SchemaEvolution` con `add_column`.
- Integración futura: DROP COLUMN, RENAME, ALTER TYPE.

## Estado del arte

- **PostgreSQL**: `ALTER TABLE ADD/DROP/RENAME COLUMN`.
- **SQLite**: `ALTER TABLE ADD COLUMN` (limitado).
- **RuscaDB actual**: migraciones forward-only (SPEC-0077).

## Diseño propuesto

1. **add_column**: añade una columna a una tabla existente.
2. **drop_column**: elimina una columna (futuro).
3. **rename_column**: renombra una columna (futuro).

## Notas

- Implementación inicial: add_column stub.
- drop_column y rename_column quedan como trabajo futuro.
---
