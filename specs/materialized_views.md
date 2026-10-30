---
id: SPEC-0149
feature: materialized_views
status: implemented
maturity: real
maturity: stub
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb/src/materialized_view.rs]
  out_of_scope: [refresh automatico, vistas sobre multiples tablas]
fr:
  - { id: FR-0149-01, desc: "feature flag `materialized-views` (off por defecto) habilita vistas materializadas" }
  - { id: FR-0149-02, desc: "MaterializedView::create materializa el resultado de una consulta" }
nf:
  - { id: NF-0149-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0149-01
    given: la feature `materialized-views` activada
    when: se crea una vista materializada
    then: la vista contiene los datos de la consulta
    test: test_ac_0149_01_materialized_view_create
exit_criteria:
  - cargo test -p ruscadb --all-features --test materialized_view
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0149 â€” Vistas materializadas

## Contexto

Frontier 2026 (PostgreSQL, TimescaleDB): vistas materializadas pre-calculan
resultados de consultas costosas. RuscaDB no tiene vistas. Esta spec aÃ±ade
vistas materializadas bÃ¡sicas detrÃ¡s de un feature flag.

## Superficie

- Feature `materialized-views` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/materialized_view.rs` â€” `MaterializedView` con `create` + `refresh`.
- IntegraciÃ³n futura: refresh automÃ¡tico, vistas sobre mÃºltiples tablas.

## Estado del arte

- **PostgreSQL**: `CREATE MATERIALIZED VIEW`, `REFRESH MATERIALIZED VIEW`.
- **TimescaleDB**: continuous aggregates (materialized views automÃ¡ticas).
- **RuscaDB actual**: sin vistas.

## DiseÃ±o propuesto

1. **create**: ejecuta la consulta y almacena el resultado como una tabla.
2. **refresh**: re-ejecuta la consulta y reemplaza los datos.
3. **drop**: elimina la vista.

## Notas

- ImplementaciÃ³n inicial: materializaciÃ³n en memoria.
- Persistencia y refresh automÃ¡tico quedan como trabajo futuro.
---
