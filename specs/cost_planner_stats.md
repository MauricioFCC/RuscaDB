---
id: SPEC-0201
feature: cost_planner_stats
status: implemented
maturity: real
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-query]
  files:
    - crates/ruscadb/src/planner_cost.rs
    - crates/ruscadb/src/catalog.rs
    - crates/ruscadb/src/executor/plan.rs
    - crates/ruscadb/src/table_api.rs
    - crates/ruscadb/tests/cost_planner_stats.rs
  out_of_scope: [histogramas multidimensionales, re-optimización adaptativa, estadísticas multi-columna, coste de agregados]
fr:
  - { id: FR-0201-01, desc: "Database::analyze recolecta filas, nulos, distintos, min/max e histograma por columna" }
  - { id: FR-0201-02, desc: "las estadisticas se persisten de forma atomica y se invalidan al mutar la tabla" }
  - { id: FR-0201-03, desc: "el planner estima selectividad y elige scan/join por coste, no por heuristica fija" }
  - { id: FR-0201-04, desc: "EXPLAIN expone estimated_rows, coste y estrategia elegida" }
nf:
  - { id: NF-0201-01, desc: "recoleccion O(N) en una pasada; memoria O(C·B) (C columnas, B buckets)" }
  - { id: NF-0201-02, desc: "sin estadisticas o stale: fallback conservador al heuristico actual, nunca error" }
  - { id: NF-0201-03, desc: "determinista: mismas filas => mismas estadisticas y mismo plan" }
acceptance_criteria:
  - id: AC-0201-01
    given: una tabla con filas, NULLs y valores repetidos
    when: se ejecuta Database::analyze
    then: rows/nulls/distinct/min/max coinciden con el oraculo (conteo exacto)
    test: test_ac_0201_01_stats_collect
  - id: AC-0201-02
    given: estadisticas de una columna con histograma
    when: se estima la selectividad de igualdad y rango
    then: el error absoluto es <= 10 % relativo frente a la frecuencia empirica
    test: test_ac_0201_02_selectivity_estimate
  - id: AC-0201-03
    given: una igualdad muy selectiva y otra poco selectiva
    when: se planifica la consulta
    then: la selectiva usa IndexScan y la poco selectiva usa SeqScan (EXPLAIN lo muestra)
    test: test_ac_0201_03_plan_choice
  - id: AC-0201-04
    given: un join equi con y sin indice interior y cardinalidades conocidas
    when: se planifica y ejecuta
    then: el planner elige IndexNestedLoop o HashJoin por coste y EXPLAIN lo declara
    test: test_ac_0201_04_join_strategy_cost
exit_criteria:
  - cargo test -p ruscadb --test cost_planner_stats
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - desactivar el coste (feature/flag) y volver al heuristico; las stats son un sidecar regenerable
sandbox:
  - cargo test -p ruscadb --test cost_planner_stats -- --nocapture
---
# SPEC-0201 — Planner por coste y estadísticas de tabla

## Contexto

El plan actual solo elige `IndexScan` si la primera columna indexada aparece en
el `WHERE`; el resto es `SeqScan`. El join elige índice interior si existe y
hash join si no (SPEC-0200), sin comparar costes. `planner_cost.rs` existe como
estimador aislado, pero no hay estadísticas ni decisión por coste. En tablas
grandes esto produce planes malos (scan completo para igualdades selectivas o
índice para predicados poco selectivos).

## Diseño

1. **Estadísticas** (`TableStats`): filas, y por columna `nulls`, `distinct`,
   `min`, `max` y un histograma equi-ancho de `B` buckets (B=32 por defecto).
2. **`ANALYZE` por API** (`Database::analyze`):
   - una pasada `O(N)` sobre filas visibles;
   - persistencia atómica en `<data>.stats.json` (tmp + rename);
   - marca `stale=true` en INSERT/UPDATE/DELETE/ALTER; el planner usa el
     heuristico mientras esté stale.
3. **Selectividad**:
   - igualdad: `1/distinct` (ajustada por `nulls`);
   - rango: fracción del histograma entre cotas + interpolación lineal;
   - `IS NULL`: `nulls/rows`.
4. **Modelo de coste** (constantes calibrables por microbench):
   ```text
   seq_scan      = rows · c_seq
   index_scan    = log2(rows) · c_idx + rows·sel · c_rand
   hash_join     = inner + outer + output
   index_nl_join = outer · (log2(inner) + matches)
   nested_loop   = outer · inner
   ```
   El planner elige `argmin(cost)`; sin stats usa el heuristico actual.
5. **EXPLAIN** añade `estimated_rows`, `cost` y `strategy`.

## Complejidad

```text
ANALYZE:   O(N) tiempo, O(C·B) memoria
planning:  O(1) heuristico -> O(P + C) con stats (P predicados)
```

## TDD adversarial

- Oráculo congelado: conteo exacto de filas/nulos/distintos/min/max.
- Selectividad vs frecuencia empírica (error ≤ 10 %).
- Plan choice con dos selectividades opuestas.
- Join strategy con cardinalidades que cruzan el umbral índice↔hash.
- PBT: mismas filas en distinto orden ⇒ mismas stats y mismo plan.

## Riesgos

- Stats stale tras escrituras: fallback conservador y `stats_stale` visible en EXPLAIN.
- Histograma sesgado: B configurable; se documenta el error esperado.
- Constantes de coste: se calibran con benchmarks deterministas, no wall-clock en CI.

## Estado

Implementada (2026-11-04). `stats.rs` calcula stats en una pasada y persiste el
sidecar; `plan_for` usa coste para elegir `IndexScan`/`FullScan`; `join.rs`
elige `IndexNestedLoop`/`HashJoin`; `EXPLAIN` añade `ESTIMATED_ROWS`, `COST`,
`STRATEGY` y `STATS`. La invalidación cubre INSERT/UPDATE/DELETE (no ALTER).
Tests AC-0201-01..04 en `crates/ruscadb/tests/cost_planner_stats.rs`.
