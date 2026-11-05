---
id: SPEC-0203
feature: plan_cache
status: implemented
maturity: real
owner: query-team
appetite_days: 3
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/database.rs
    - crates/ruscadb/src/table_api.rs
    - crates/ruscadb/src/executor/execute.rs
    - crates/ruscadb/tests/plan_cache.rs
  out_of_scope: [planes genéricos parametrizados, caché entre sesiones, caché de resultados]
fr:
  - { id: FR-0203-01, desc: "SELECT simples reutilizan el plan de acceso sin re-planificar" }
  - { id: FR-0203-02, desc: "DML, ANALYZE y DDL invalidan la caché vía época de mutaciones" }
  - { id: FR-0203-03, desc: "caché acotada (64 entradas) con evicción LRU y contadores hits/misses" }
nf:
  - { id: NF-0203-01, desc: "lookup O(1) esperado; memoria O(C·plan), C=64" }
  - { id: NF-0203-02, desc: "sin dependencias nuevas; resultado idéntico al camino sin caché" }
  - { id: NF-0203-03, desc: "EXPLAIN no consume la caché; siempre muestra el plan actual" }
acceptance_criteria:
  - id: AC-0203-01
    given: una consulta SELECT repetida
    when: se ejecuta dos veces
    then: la segunda es un hit y el resultado es idéntico
    test: test_ac_0203_01_plan_cache_hit
  - id: AC-0203-02
    given: un plan cacheado
    when: se hace INSERT, ANALYZE o CREATE INDEX
    then: la siguiente consulta re-planifica y ve el estado nuevo
    test: test_ac_0203_02_plan_cache_invalidates
  - id: AC-0203-03
    given: más consultas distintas que la capacidad
    when: se ejecutan 100 SELECT
    then: la caché no supera 64 entradas (LRU)
    test: test_ac_0203_03_plan_cache_is_bounded
exit_criteria:
  - cargo test -p ruscadb --test plan_cache
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - desactivar la caché (flag) y volver a planificar siempre; no cambia el formato
sandbox:
  - cargo test -p ruscadb --test plan_cache -- --nocapture
---
# SPEC-0203 — Caché de planes acotada

## Contexto

Tras SPEC-0201 el planner elige por coste, pero cada `execute` vuelve a cargar
el catálogo y a planificar. En bucles de lectura esto desperdicia trabajo.

## Diseño

1. `Database` mantiene `plan_cache: BTreeMap<String, CachedPlan>` + LRU.
2. En `execute_dispatch`, SELECT sin tabla derivada: hit con época actual ⇒
   ejecutar con el plan; miss ⇒ `plan_for` y almacenar.
3. `mutation_epoch` se incrementa en DML, `analyze`, `create_table`,
   `create_index` y `alter_table_add_column`; `analyze` además limpia la caché.
4. `EXPLAIN` no consulta la caché.

## Complejidad

```text
lookup: O(1) esperado (+ O(C) al mover el LRU)
memoria: O(C · tamaño del plan), C = 64
invalidación: O(1)
```

## TDD adversarial

Hit/miss con resultado idéntico; invalidación por INSERT/ANALYZE/DDL; cota con
100 consultas distintas.

## Riesgos

Plan obsoleto si una mutación no incrementa la época; las rutas actuales lo
hacen. SELECT con tabla derivada/CTE no se cachea.
