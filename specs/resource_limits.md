---
id: SPEC-0189
feature: resource_limits
status: implemented
maturity: real
owner: core-team
appetite_days: 2
boundaries:
  crates: [ruscadb]
  files: [crates/ruscadb/src/database.rs, crates/ruscadb/src/table_api.rs]
  out_of_scope: [enforcement de memoria, timeout cooperativo por deadline]
fr:
  - { id: FR-0189-01, desc: "tope de filas por query en DbConfig" }
nf:
  - { id: NF-0189-01, desc: "sin dependencias nuevas" }
acceptance_criteria:
  - id: AC-0189-01
    given: DbConfig con max_result_rows
    when: se ejecuta un SELECT con mas filas
    then: el resultado se trunca al tope
    test: test_ac_0189_01_max_result_rows
exit_criteria:
  - cargo test -p ruscadb --all-features --test spec_0189_limits
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir DbConfig/table_api
sandbox:
  - cargo test -p ruscadb --all-features --test spec_0189_limits
---

# SPEC-0189 — Límites de recursos (P2.4)

## Implementado

- `DbConfig::max_result_rows: Option<usize>`: trunca cualquier resultado de
  `execute`/`execute_at` (incluye set ops y DML con `RETURNING`).

## Reservado (documentado, no activo)

- `query_timeout_ms` y `max_memory_bytes`: aceptados en la configuración para
  no romper consumidores; su enforcement requiere deadline por operador y
  presupuesto de pool respectivamente.

## Complejidad

Truncado: `O(1)` (solo cambia la longitud del `Vec`).
