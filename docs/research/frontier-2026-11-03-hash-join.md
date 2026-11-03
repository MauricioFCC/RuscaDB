# Frontera de ejecución: hash join (2026-11-03)

> Tarea: análisis de gaps de RuscaDB y aplicación de la mejora de mayor ROI
> detectada (join de igualdad sin índice). Ciclo `frontier-engineering`.

## 1. Load

- `crates/ruscadb/src/join.rs`: `execute_join_at` usa índice B+tree si el lado
  interior lo tiene; si no, nested loop `O(n·m)`.
- `docs/MVP.md`/`docs/RuscaDB-funcionalidades.md`: "sin optimizador; solo
  IndexScan"; el join sin índice quedaba fuera.
- SPEC-0108 (join con WHERE/ORDER BY) y SPEC-0103 (RIGHT/FULL/CROSS) cubren
  semántica, no coste.

## 2. Research SOTA — fuentes leídas

| Fuente | URL | Aporte |
|---|---|---|
| PostgreSQL 18 — Planner/Optimizer | https://www.postgresql.org/docs/current/planner-optimizer.html | el planner elige nested loop, hash join o merge join según coste |
| Hash join — Wikipedia | https://en.wikipedia.org/wiki/Hash_join | build/probe, Grace/hybrid hash join, memoria y spill |
| DuckDB docs (join) | https://duckdb.org/docs/stable/guides/performance/how_to_tune_workloads | **[UNVERIFIED]** no leída en esta sesión |
| CMU 15-721 (join algorithms) | https://15721.courses.cs.cmu.edu/spring2024/ | **[UNVERIFIED]** no leída en esta sesión |

## 3. Hipótesis comparadas

| # | Estrategia | Accuracy | Latencia esperada | Memoria | Robustez | Reproducibilidad |
|---|---|---|---|---|---|---|
| H1 | Nested loop (actual sin índice) | Alta | O(n·m) | O(1) | Alta | Alta |
| H2 | Hash join (elegida) | Alta | O(n+m) | O(m) | Alta | Alta |
| H3 | Sort-merge join | Alta | O(n log n + m log m) | O(1)–O(n+m) | Alta | Alta |
| H4 | Índice B+tree (ya existe) | Alta | O(n log m) | O(1) | Alta | Alta |

**Decisión:** H2 como fallback cuando no hay índice; H4 se conserva para claves
selectivas. H3 se difiere: exige ordenar ambos lados y no mejora a H2 en este
motor (sin spill a disco).

## 4. Diseño

```text
build:  HashMap<JoinKey, Vec<&Record>> sobre filas interiores visibles
probe:  una búsqueda por fila exterior; conserva orden de buckets
JoinKey: Number(bits f64) | Text | Bool
  - Int/UInt/Float normalizados igual que keys_equal
  - -0.0 -> 0.0; NaN/NULL/Bytes/Timestamp => sin clave (nunca emparejan)
```

Complejidad: `O(n·m) → O(n+m+k)` esperado; memoria `O(m)`.

## 5. Gap analysis

| Gap | Impacto | Acción |
|---|---|---|
| Equi-join sin índice O(n·m) | bloquea tablas medianas (10k×10k) | hash join (SPEC-0200) |
| Sin spill a disco | hash join puede consumir O(m) | aceptado para el MVP; `max_memory_bytes` reservado |
| Sin join de 3+ tablas / no-equi | fuera de alcance | documentado |

## 6. No aplicado

- **Grace/hybrid hash join con particiones**: requiere spill y presupuesto de
  memoria real; el motor aún materializa resultados.
- **Sort-merge join**: coste de ordenación no compensa sin índices ordenados.
- **Estadísticas/coste**: el planner sigue eligiendo solo IndexScan; un
  optimizador basado en coste es una ola aparte.
