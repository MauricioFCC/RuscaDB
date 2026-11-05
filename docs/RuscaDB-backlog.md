# Backlog real de RuscaDB — decisión de ciclo (2026-11-03)

> Cierre del ciclo frontier. Cada ítem tiene spec, diseño, ACs, complejidad y
> **motivo de no implementación** en este ciclo. Nada se marca `real` sin tests
> SQL ejecutables.

## Matriz de decisión

| # | Ítem | Spec | Estado | Bloqueo real | Decisión |
|---|---|---|---|---|---|
| D1 | `FROM (subquery) AS alias` | SPEC-0162 | implemented/real | — | implementado con `VirtualTable` y `FromClause` derivado |
| D2 | CTE `WITH ... AS (...)` | SPEC-0163 | implemented/real | — | implementado, incluido `WITH RECURSIVE` |
| D3 | Window `OVER (PARTITION/ORDER)` | SPEC-0165 | implemented/real | — | implementado `Projection` con ventanas y `WindowSpec` |
| D4 | `RELATE` + paths `->label->` | SPEC-0185 | implemented/real | — | implementado `Statement::Relate` + paths BFS encadenados |
| D5 | `LET`/`IF`/`RETURN` | SPEC-0186 | implemented/real | — | implementado `execute_script` con contexto por llamada |
| D6 | ML-KEM FIPS 203 | SPEC-0180 | implemented/real | auditoría externa pendiente (feature off por defecto) | implementado con libcrux-ml-kem (hax/F*) + KAT ACVP; híbrido X25519 y keystore |
| D7 | Payload JSON + `ts_unix` en aristas | SPEC-0193 | implemented/real | — | implementado: tabla interna `__ruscadb_edge_meta` + índice `edge_id`, `SET`/`AT`/`ON CONFLICT` y proyección `{payload, ts_unix}` |

## Diseño y complejidad por ítem

### D1/D2 — Tabla virtual + CTE

```text
materializar(subquery) = O(m·k)      (m filas, k ancho)
escaneo outer          = O(m)
total                  = O(m·k)      vs O(m²) si se re-ejecuta por fila
```

Fase 1: `VirtualTable { columns, rows }` y un plan `Plan::VirtualScan`.
Fase 2: `WITH` resuelve nombres a `VirtualTable` en un `BTreeMap`.
ACs: `FROM (SELECT ...) AS t` devuelve filas; `WITH t AS (...) SELECT * FROM t`;
dos CTE encadenadas; error si el alias no existe.

### D3 — Window functions

```text
ROW_NUMBER (ORDER BY)      O(n log n)  (sort estable)
SUM OVER (PARTITION BY)    O(n log n)  o O(n) con prefijos por partición
```

Fase 1: `ROW_NUMBER() OVER (ORDER BY col) AS rn` post-proyección.
Fase 2: `PARTITION BY` + `SUM/AVG` con prefix sums.

### D4 — RELATE + paths

```text
RELATE                 O(log n) por arista (índice in,out,label)
path BFS               O(V + E) con dedupe BTreeSet
producto de grados     prohibido (O(∏ deg) )
```

Fase 1: tabla de aristas `{in,out,label,data}` + `RELATE`.
Fase 2: `SELECT ->label->tabla` BFS por niveles.

### D5 — LET/IF/RETURN

```text
LET     O(1) por variable (BTreeMap)
IF      O(1) + rama elegida
script  O(sentencias) con el mismo snapshot MVCC
```

Fase 1: `execute_script`, `LET $x = literal`, `RETURN`.
Fase 2: variables en `WHERE` (`Expr::Variable`) e `IF` en proyección.

### D6 - ML-KEM

Implementado (SPEC-0180): `libcrux-ml-kem` 0.0.11 (formalmente verificado con
hax/F*) para ML-KEM-768, híbrido X25519+ML-KEM con SHA-256 y keystore
`pq_keys`. KAT ACVP/FIPS 203 en `crates/ruscadb-crypto/tests/post_quantum.rs`.
**Riesgo residual**: no hay auditoría externa independiente; la feature
`post-quantum` sigue OFF por defecto.

### D7 - Payload JSON + `ts_unix` en aristas

Spec `SPEC-0193` (`specs/edge_properties.md`) **implementada**. Tabla interna
`__ruscadb_edge_meta` con clave sintética `edge_id = SHA-256(from,label,to)` e
índice B+tree; `Record.edges` no cambia. Idempotencia estricta con
`ON CONFLICT DO NOTHING | DO UPDATE SET payload`; `ts_unix` inmutable.
7 tests SQL en `crates/ruscadb/tests/edge_properties.rs`.

## Gaps de madurez cerrados (SPEC-0194..0198, 2026-11-03)

| # | Gap | Spec | Evidencia | Prioridad |
|---|---|---|---|---|
| G1 | `O(n²)` por `Vec::contains` en gc/free-list/columnar | SPEC-0194 | ✅ implementado (`BTreeSet`/`HashSet`) | Alta |
| G2 | `TxnManager::aborted` sin reclaim | SPEC-0195 | ✅ implementado (reap purga y olvida) | Alta |
| G3 | py/node sin conformance (fase 2) | SPEC-0196 | ✅ implementado (fixture tipado + hash JCS común + tamper gate en CI; WASM parse) | Media |
| G4 | Métricas sin endpoint HTTP | SPEC-0197 | ✅ implementado (`metrics-http`) | Media |
| G5 | Sin benchmarks macro (ANN/LDBC/SQL) | SPEC-0198 | ✅ implementado (recall@10, grafos, SQL y `bench-gate` endurecido) | Media |

Informe de madurez: `docs/RuscaDB-maturity.md`. Research:
`docs/research/frontier-2026-11-02-ruscadb-maturity.md`.

## Criterio de aceptación de este cierre

- **205 specs**: 205 `implemented` (0 `proposed`);
  `cargo xtask audit` en 0 ACs sin test y **680 ACs trazados**.
- Cada fase conserva ACs, complejidad y tests SQL; todas las specs
  implementadas tienen gates verdes y tests activos (sin scaffolds `#[ignore]`).
- Lo no aplicado sigue aquí con motivo (auditoría externa PQC, KMS/HSM,
  TLS, LIVE SELECT), no como deuda invisible.

## Alternativas evaluadas y diferidas (2026-11-03)

Aplicadas ya: **hash join** para igualdad sin índice (SPEC-0200) y **gate de
documentación** con anclas (SPEC-0199). El resto se evalúa aquí para que no
quede solo en los research docs:

| Alternativa | Decisión | Coste | Disparador para retomarla |
|---|---|---|---|
| Hash join con spill (Grace/híbrido) | Diferida | Alto | `max_memory_bytes` real y `m > presupuesto` |
| Sort-merge join | Rechazada | Medio | entradas ya ordenadas y necesidad de memoria O(1) |
| Optimizador por coste + estadísticas | Implementado (SPEC-0201) | Alto | stats + planner + EXPLAIN con coste |
| Plan cache / correlacionadas / laterales | Plan cache implementado (SPEC-0203); correlacionadas/laterales pendientes | Alto | siguiente ola de query |
| ANALYZE en RQL | Implementado (SPEC-0206) | Bajo | stats refrescables desde SQL |
| Release/instalación | Implementado (SPEC-0208): validador offline, fallback git, docs `--dry-run` | Bajo | crear tag `v*` para publicar |
| DROP INDEX / DROP TABLE | Implementados en RQL (SPEC-0205/0207/0209); reclamación de páginas y CASCADE pendientes | Medio | siguiente ola de schema |
| Streaming / cursores con estado | Streaming simple implementado (SPEC-0210); cursores reanudables y streaming de JOIN/agregados/ORDER BY pendientes | Medio | resultados mayores que el presupuesto de memoria |
| JOIN de 3+ tablas y no-equi | Diferida | Alto | demanda real; hoy 2 tablas + igualdad |
| Servidor pgwire TCP | Diferida | Alto | adapter de servidor (fuera del core embebido) |
| TLS/auth en `metrics-http` | Implementado (SPEC-0202) | Medio | Bearer + rustls con fail-closed |
| KMS/HSM externo | Fuera de alcance | Alto | requisito de custodia externa |
| mdBook/MkDocs | Diferida | Medio | publicación de documentación externa |
| Vale/spellcheck y traducción EN | Diferida | Medio | documentación pública en inglés |

Research: `docs/research/frontier-2026-11-03-docs.md` y
`docs/research/frontier-2026-11-03-hash-join.md`.

## Referencias

- SurrealDB RELATE/graph docs: `docs/research/surrealdb-2026-11-02.md`.
- Ngo et al. 2018; Malkov & Yashunin 2020; O'Neil et al. 1996.
- PostgreSQL 18 `WITH`/window docs; SQL:2023.
