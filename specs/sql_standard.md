---
id: SPEC-0141
feature: sql_standard
status: proposed
owner: query-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb/src/sql_std.rs]
  out_of_scope: [full SQL:2023 compliance, stored procedures, triggers]
fr:
  - { id: FR-0141-01, desc: "feature flag `sql-std` (off por defecto) habilita extensiones SQL estandar" }
  - { id: FR-0141-02, desc: "WindowFunc::row_number asigna numero de fila por particion" }
nf:
  - { id: NF-0141-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0141-01
    given: la feature `sql-std` activada
    when: se calcula row_number sobre una particion
    then: los numeros son secuenciales por particion
    test: test_ac_0141_01_row_number
exit_criteria:
  - cargo test -p ruscadb --all-features --test sql_std
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0141 — SQL estándar (window functions, CTEs, MERGE)

## Contexto

Frontier 2026 (DuckDB, PostgreSQL): SQL completo con window functions, CTEs,
MERGE. RuscaDB tiene RQL acotado. Esta spec añade extensiones SQL estándar
detrás de un feature flag.

## Superficie

- Feature `sql-std` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/sql_std.rs` — `WindowFunc` con `row_number`.
- Integración futura: MERGE, CTEs, window functions completas.

## Estado del arte

- **DuckDB**: SQL completo, window functions, CTEs, MERGE.
- **PostgreSQL**: SQL:2023, window functions, CTEs, MERGE.
- **RuscaDB actual**: RQL acotado (sin window functions, CTEs, MERGE).

## Diseño propuesto

1. **Window functions**: `ROW_NUMBER()`, `RANK()`, `DENSE_RANK()`, `LAG()`, `LEAD()`.
2. **CTEs**: `WITH ... AS (...) SELECT ...`.
3. **MERGE**: `MERGE INTO ... WHEN MATCHED ... WHEN NOT MATCHED ...`.

## Notas

- Implementación inicial: row_number stub.
- Window functions completas, CTEs y MERGE quedan como trabajo futuro.
---
