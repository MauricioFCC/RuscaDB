---
id: SPEC-0190
feature: conformance_fixtures
status: implemented
maturity: real
owner: core-team
appetite_days: 2
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/tests/conformance/basic.json
    - crates/ruscadb/tests/spec_0190_conformance.rs
  out_of_scope: [ejecucion en rusadb-py/node/wasm (fase 2)]
fr:
  - { id: FR-0190-01, desc: "fixtures SQL+resultado canonico ejecutables por bindings" }
nf:
  - { id: NF-0190-01, desc: "JSON estable y sin dependencias nuevas" }
acceptance_criteria:
  - id: AC-0190-01
    given: el fixture conformance/basic.json
    when: se ejecuta en Rust
    then: cada consulta produce exactamente el resultado canonico
    test: test_ac_0190_01_conformance_fixtures
exit_criteria:
  - cargo test -p ruscadb --all-features --test spec_0190_conformance
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir fixture/test
sandbox:
  - cargo test -p ruscadb --all-features --test spec_0190_conformance
---

# SPEC-0190 — Conformance baseline multi-binding (P2.7)

## Implementado

- `tests/conformance/basic.json`: casos `{name, sql, expect}` con resultados
  canónicos (filas como objetos JSON).
- Runner Rust que ejecuta el fixture y compara exacto.

## Fase 2 (pendiente)

- Ejecutar el mismo fixture desde `ruscadb-py`, `ruscadb-node` y `ruscadb-wasm`
  en CI y comparar un hash del JSON.
- Añadir casos de tipos (NULL, Float, Text, Bool) y errores.

## Complejidad

Runner: `O(casos · filas)`; sin dependencias nuevas.
