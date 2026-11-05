# Frontera: EXPLAIN ANALYZE y cardinalidad de join (2026-11-04)

> Cierre del bucle planner↔ejecución tras SPEC-0201 (coste) y SPEC-0203 (plan
> cache). Objetivo: medir filas/tiempo reales y mejorar la estimación de joins
> con estadísticas.

## 1. Research SOTA — fuentes leídas

| Fuente | URL | Aporte |
|---|---|---|
| PostgreSQL 18 — `EXPLAIN` | https://www.postgresql.org/docs/current/sql-explain.html | `ANALYZE` ejecuta y añade filas/tiempo reales; `BUFFERS` fuera de alcance |
| PostgreSQL 18 — Planner/Optimizer | https://www.postgresql.org/docs/current/planner-optimizer.html | estimaciones de cardinalidad y coste por operador |
| CMU 15-721 (query execution) | https://15721.courses.cs.cmu.edu/spring2024/ | **[UNVERIFIED]** no leída en esta sesión |

## 2. Hipótesis comparadas

| # | Estrategia | Información | Coste | Reproducibilidad |
|---|---|---|---|---|
| H1 | Solo `EXPLAIN` (estimaciones) | baja | mínimo | alta |
| H2 | `EXPLAIN ANALYZE` con filas/tiempo | alta | 1 ejecución | media (wall-clock) |
| H3 | Perfil por operador + buffers/IO | máxima | alto (instrumentación) | media |
| H4 | Muestreo estadístico de queries | media | bajo | baja |

**Decisión:** H2 (filas + tiempo total) para cerrar el bucle sin instrumentar el
executor. H3 queda para una ola de profiling. La cardinalidad de join usa
`filas_izq·filas_der/max(distinct)` (contención), no un producto cartesiano.

## 3. Diseño

```text
EXPLAIN ANALYZE: coste = plan + execute + O(1);  memoria = O(lineas)
join estimate:   O(1) con stats; fallback O(1) sin stats
```

- `Database::explain_analyze` valida `EXPLAIN SELECT`, ejecuta una vez y añade
  `ACTUAL_ROWS`/`ACTUAL_TIME_MS`.
- `TableStats::join_cardinality` usa no-nulos y `max(distinct)`.
- `EXPLAIN` de join añade `JOIN_ESTIMATED_ROWS`.

## 4. Gap analysis

| Gap | Acción |
|---|---|
| Sin filas/tiempo reales | `explain_analyze` |
| Join estimado como producto cartesiano | cardinalidad con distinct |
| Plan cache invalidada por ANALYZE | ANALYZE limpia solo la caché, no las stats ajenas |

## 5. No aplicado

- **Per-operator timing / buffers**: requiere instrumentar el executor y un
  formato de perfil; siguiente ola.
- **`EXPLAIN ANALYZE` en RQL**: se expone por API para no tocar el parser.
- **Histogramas multi-columna**: fuera de alcance; la independencia es una
  aproximación documentada.
