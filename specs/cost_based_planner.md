---
id: SPEC-0142
feature: cost_based_planner
status: implemented
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb/src/planner_cost.rs]
  out_of_scope: [distributed, full optimizer, join reordering]
fr:
  - { id: FR-0142-01, desc: "feature flag `cost-planner` (off por defecto) habilita planificador basado en costos" }
  - { id: FR-0142-02, desc: "CostEstimator::estimate devuelve un costo para una operacion" }
nf:
  - { id: NF-0142-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0142-01
    given: la feature `cost-planner` activada
    when: se estima el costo de un scan
    then: el costo es mayor para tablas mas grandes
    test: test_ac_0142_01_cost_estimate
exit_criteria:
  - cargo test -p ruscadb --all-features --test cost_planner
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0142 — Planificador basado en costos

## Contexto

Frontier 2026 (PostgreSQL, DuckDB): optimizador de consultas basado en costos
con estadísticas. RuscaDB no tiene optimizador (solo IndexScan por igualdad).
Esta spec añade un estimador de costos básico detrás de un feature flag.

## Superficie

- Feature `cost-planner` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/planner_cost.rs` — `CostEstimator` con `estimate`.
- Integración futura: estadísticas, join reordering, cost-based plan selection.

## Estado del arte

- **PostgreSQL**: cost-based optimizer con estadísticas (pg_statistic).
- **DuckDB**: cost-based optimizer con histograms.
- **RuscaDB actual**: sin optimizador (solo IndexScan por igualdad).

## Diseño propuesto

1. **CostEstimator**: estima costo de scan, filter, join, sort.
2. **Estadísticas**: row count, selectivity, cardinality.
3. **Plan selection**: elige el plan de menor costo.

## Notas

- Implementación inicial: estimador lineal (costo = rows * factor).
- Estadísticas reales y join reordering quedan como trabajo futuro.
---
