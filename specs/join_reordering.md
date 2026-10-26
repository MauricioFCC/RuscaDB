---
id: SPEC-0153
feature: join_reordering
status: implemented
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb/src/join_reorder.rs]
  out_of_scope: [distributed, full optimizer]
fr:
  - { id: FR-0153-01, desc: "feature flag `join-reorder` (off por defecto) habilita reordenamiento de joins" }
  - { id: FR-0153-02, desc: "JoinReorderer::reorder elige el orden de menor costo" }
nf:
  - { id: NF-0153-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0153-01
    given: la feature `join-reorder` activada
    when: se reordenan dos tablas
    then: la tabla mas pequena va primero
    test: test_ac_0153_01_join_reorder
exit_criteria:
  - cargo test -p ruscadb --all-features --test join_reorder
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0153 — Reordenamiento de joins (cost-based)

## Contexto

Frontier 2026 (PostgreSQL, DuckDB): reordenamiento de joins por costo.
RuscaDB ejecuta joins en el orden escrito. Esta spec añade reordenamiento
básico detrás de un feature flag.

## Superficie

- Feature `join-reorder` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/join_reorder.rs` — `JoinReorderer` con `reorder`.
- Integración futura: cost-based plan selection, estadísticas.

## Estado del arte

- **PostgreSQL**: join reordering con cost-based optimizer.
- **DuckDB**: join reordering con statistics.
- **RuscaDB actual**: sin reordenamiento (orden escrito).

## Diseño propuesto

1. **JoinReorderer**: estima el costo de cada orden y elige el menor.
2. **Estadísticas**: row count por tabla.
3. **Heurística**: tabla más pequeña primero (inner join).

## Notas

- Implementación inicial: heurística simple (tabla más pequeña primero).
- Cost-based real queda como trabajo futuro.
---
