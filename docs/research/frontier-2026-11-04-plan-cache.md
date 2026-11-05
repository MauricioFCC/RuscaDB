# Frontera: caché de planes de consulta (2026-11-04)

> Ola siguiente tras el planner por coste (SPEC-0201). Objetivo: reutilizar el
> plan de acceso sin re-planificar en cargas de lectura repetitivas, con
> invalidación correcta ante DML/DDL/stats.

## 1. Research SOTA — fuentes leídas

| Fuente | URL | Aporte |
|---|---|---|
| PostgreSQL 18 — `PREPARE` | https://www.postgresql.org/docs/current/sql-prepare.html | planes genéricos vs personalizados; re-planificación y `plan_cache_mode` |
| PostgreSQL 18 — Planner/Optimizer | https://www.postgresql.org/docs/current/planner-optimizer.html | el plan depende de estadísticas y del coste |
| DuckDB prepared statements | https://duckdb.org/docs/stable/sql/query_syntax/prepare | **[UNVERIFIED]** no leída (404 en la ruta probada) |
| CMU 15-721 (query execution) | https://15721.courses.cs.cmu.edu/spring2024/ | **[UNVERIFIED]** no leída en esta sesión |

## 2. Hipótesis comparadas

| # | Estrategia | Latencia | Memoria | Corrección | Complejidad |
|---|---|---|---|---|---|
| H1 | Sin caché (re-planificar siempre) | alta en bucles | O(1) | alta | baja |
| H2 | Caché de AST (solo parse) | media | O(C·AST) | alta | baja |
| H3 | Caché de plan con época (elegida) | baja | O(C·plan) | alta con invalidación | media |
| H4 | Caché de resultados (semántica) | mínima con hit | O(resultados) | riesgo de staleness | alta |

**Decisión:** H3 con clave `SQL` + `mutation_epoch`; capacidad 64 y LRU. Los
cambios de datos/estadísticas/DDL incrementan la época y fuerzan re-planificar.
H4 ya existe como caché semántica separada y no se mezcla con el planner.

## 3. Diseño

```text
lookup:   O(1) esperado (BTreeMap) + O(C) para el LRU (C=64)
memoria:  O(C · tamaño del plan)
invalidación: O(1) por comparación de época
```

- `CachedPlan { plan, epoch }`.
- `mutation_epoch` se incrementa en INSERT/UPDATE/DELETE, `analyze`, `CREATE
  TABLE`, `CREATE INDEX` y `ALTER TABLE`.
- `EXPLAIN` no usa la caché (siempre planifica y muestra el estado actual).

## 4. Gap analysis

| Gap | Acción |
|---|---|
| Re-planificación en bucles de lectura | caché LRU de plan |
| Plan obsoleto tras DML/DDL/stats | invalidación por época |
| Memoria sin cota | capacidad 64 + evicción LRU |

## 5. No aplicado

- **Planes genéricos parametrizados** (PostgreSQL `plan_cache_mode`): requiere
  parámetros vinculados y `EXPLAIN` con placeholders; siguiente ola.
- **Caché entre sesiones/procesos**: no aplica a una base embebida.
- **Caché de resultados**: ya cubierta por la caché semántica (SPEC-0065/0069).
