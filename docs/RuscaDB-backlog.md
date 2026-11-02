# Backlog real de RuscaDB — decisión de ciclo (2026-11-02)

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

## Gaps de madurez (SPEC-0194..0198, propuestas 2026-11-02)

| # | Gap | Spec | Evidencia | Prioridad |
|---|---|---|---|---|
| G1 | `O(n²)` por `Vec::contains` en gc/free-list/columnar | SPEC-0194 | ✅ implementado (`BTreeSet`/`HashSet`) | Alta |
| G2 | `TxnManager::aborted` sin reclaim | SPEC-0195 | ✅ implementado (reap purga y olvida) | Alta |
| G3 | py/node sin conformance (fase 2) | SPEC-0196 | ⏳ bloqueado por entorno (koffi/FFI/WASM) | Media |
| G4 | Métricas sin endpoint HTTP | SPEC-0197 | ✅ implementado (`metrics-http`) | Media |
| G5 | Sin benchmarks macro (ANN/LDBC/SQL) | SPEC-0198 | ⏳ siguiente loop | Media |

Informe de madurez: `docs/RuscaDB-maturity.md`. Research:
`docs/research/frontier-2026-11-02-ruscadb-maturity.md`.

## Criterio de aceptación de este cierre

- **193 specs**: 191 `implemented` + 2 `proposed` (SPEC-0196/0198);
  `cargo xtask audit` en 0 ACs sin test y **636 ACs trazados**.
- Cada fase conserva ACs, complejidad y tests SQL; las specs implementadas
  tienen gates verdes y las propuestas tienen scaffold RED (`#[ignore]`).
- Lo no aplicado sigue aquí con motivo (auditoría externa PQC, KMS/HSM,
  TLS, LIVE SELECT), no como deuda invisible.

## Referencias

- SurrealDB RELATE/graph docs: `docs/research/surrealdb-2026-11-02.md`.
- Ngo et al. 2018; Malkov & Yashunin 2020; O'Neil et al. 1996.
- PostgreSQL 18 `WITH`/window docs; SQL:2023.
