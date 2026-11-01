# Backlog real de RuscaDB — decisión de ciclo (2026-11-02)

> Cierre del ciclo frontier. Cada ítem tiene spec, diseño, ACs, complejidad y
> **motivo de no implementación** en este ciclo. Nada se marca `real` sin tests
> SQL ejecutables.

## Matriz de decisión

| # | Ítem | Spec | Estado | Bloqueo real | Decisión |
|---|---|---|---|---|---|
| D1 | `FROM (subquery) AS alias` | SPEC-0162 | accepted/stub | requiere tabla virtual en `execute_with_plan_at` (hoy `TableDef` + heap) | diferido: fase 1 = `VirtualTable` en memoria |
| D2 | CTE `WITH ... AS (...)` | SPEC-0163 | accepted/stub | depende de D1 (tabla virtual) | diferido: fase 2 tras D1 |
| D3 | Window `OVER (PARTITION/ORDER)` | SPEC-0165 | accepted/stub | proyección solo columnas/agregados; falta `Projection::Expr` | diferido: fase 1 = `ROW_NUMBER` |
| D4 | `RELATE` + paths `->label->` | SPEC-0185 | proposed/stub | no hay update genérico de `Record.edges`; falta índice `(in,out,label)` | diferido: fase 1 = tabla de aristas |
| D5 | `LET`/`IF`/`RETURN` | SPEC-0186 | proposed/stub | `execute` es una sentencia; falta contexto de sesión | diferido: fase 1 = `execute_script` |
| D6 | ML-KEM FIPS 203 | SPEC-0180 | proposed/placeholder | requiere crate auditado + ACVP (ADR-0180) | bloqueado por dependencia |

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

### D6 — ML-KEM

Bloqueado: `ml-kem` auditado + vectores ACVP. Sin red/credenciales no se
simula seguridad. Permanece `placeholder` (feature off).

## Criterio de aceptación de este cierre

- Las specs afectadas conservan `proposed`/`accepted` y **no** se declaran
  `real`; `cargo xtask audit` sigue en 0 ACs sin test.
- Cada fase tiene ACs y complejidad; la implementación se hará en ciclos
  separados con gate de equivalencia contra oráculo congelado.
- Lo no aplicado queda aquí con motivo, no como deuda invisible.

## Referencias

- SurrealDB RELATE/graph docs: `docs/research/surrealdb-2026-11-02.md`.
- Ngo et al. 2018; Malkov & Yashunin 2020; O'Neil et al. 1996.
- PostgreSQL 18 `WITH`/window docs; SQL:2023.
