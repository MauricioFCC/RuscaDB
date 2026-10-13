---
id: SPEC-0079
feature: coverage_policy
status: implemented
owner: quality-team
appetite_days: 2
boundaries:
  crates: [ruscadb, xtask]
  files: [scripts/coverage_gate.py, .github/workflows/ci.yml, docs/verification.md]
  out_of_scope: [cobertura de benchmarks/xtask, mutation score (SPEC-0059), hosting de reportes HTML, cobertura de bindings]
fr:
  - { id: FR-0079-01, desc: "el gate acepta varios --scope con umbral propio por scope (core, txn, wal) en una sola corrida" }
  - { id: FR-0079-02, desc: "la salida imprime el % de línea/rama por scope y por crate, para ver qué crate arrastra la cobertura" }
  - { id: FR-0079-03, desc: "la política (umbrales, cómo correrla local, semántica del gate) queda documentada en docs/verification.md" }
nf:
  - { id: NF-0079-01, desc: "solo stdlib de Python (consistente con SPEC-0061); sin dependencias nuevas" }
  - { id: NF-0079-02, desc: "contrato de fallo ruidoso conservado: scope sin ficheros o lcov vacío => exit 2, nunca verde silencioso" }
  - { id: NF-0079-03, desc: "el job coverage de ci.yml sigue bloqueando el merge; se amplía el alcance, no se reemplaza la lógica" }
acceptance_criteria:
  - id: AC-0079-01
    given: un lcov fixture con dos scopes (uno bajo su umbral, otro sobre el suyo)
    when: se ejecuta coverage_gate.py con ambos --scope
    then: exit 1 con el nombre del scope infractor en la salida (y exit 0 cuando todos cumplen)
    test: test_ac_0079_01_multi_scope_gate_enforces_each_threshold
  - id: AC-0079-02
    given: un lcov fixture sin registros del scope pedido
    when: se ejecuta coverage_gate.py con ese --scope
    then: exit 2 (fallo ruidoso, contrato SPEC-0061) — nunca exit 0
    test: test_ac_0079_02_empty_scope_fails_loudly
  - id: AC-0079-03
    given: ci.yml y docs/verification.md
    when: se inspeccionan
    then: el job coverage invoca el gate multi-scope y la política documenta umbrales y uso local
    test: test_ac_0079_03_policy_wired_in_ci_and_docs
exit_criteria:
  - cargo test -p xtask --test coverage_gate
  - python scripts/coverage_gate.py lcov.info --scope ruscadb-core/ --min-line 80 --min-branch 70  (run local real)
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - python scripts/check_ci_config.py
  - cargo xtask trace / contract verdes
rollback:
  - revertir coverage_gate.py a la firma mono-scope de SPEC-0061 y dejar ci.yml como estaba
sandbox:
  - cargo test -p xtask --test coverage_gate
---

# SPEC-0079 — Política de cobertura multi-scope (Ola 1 del plan de producción)

## Contexto

Auditoría *production readiness* 2026-10-12 (riesgo 6): la cobertura solo se
mide y gatea sobre `ruscadb-core` (`coverage_gate.py`, SPEC-0061, umbral
80/70); el resto del workspace (query, executor, txn, wal, fts…) puede caer
sin que nadie se entere, y no hay visibilidad de qué crate arrastra el
número global.

Frontera 2026 (§3, research T7): `cargo-llvm-cov` 0.9 con
`--fail-under-lines` como piso; gates por componente (no agregado global) y
**reporte per-crate** para atacar donde está el agujero; umbrales duros para
los crates de corrección (`core`/`txn`/`wal`) y ratchet para el resto.

## Superficie

- `scripts/coverage_gate.py`: multi-`--scope` con `--min-line/--min-branch`
  por scope, impresión per-crate dentro de cada scope, conservando los códigos
  de salida 0/1/2 y solo stdlib.
- `.github/workflows/ci.yml` (job `coverage`): ampliar la línea de invocación
  al nuevo alcance (core + txn + wal).
- `docs/verification.md`: sección de política de cobertura (umbrales, runbook
  local, semántica de exit codes).
- Test: `crates/xtask/tests/coverage_gate.rs` invoca el script sobre fixtures
  lcov commiteados y asienta los exit codes (intérprete: `python`, con
  fallback `python3`; sin intérprete el test falla con mensaje claro).

## Criterios de aceptación

- **AC-0079-01** — gate multi-scope: cada scope con su umbral; el infractor
  nombra en la salida.
- **AC-0079-02** — scope vacío => exit 2 (nunca verde silencioso).
- **AC-0079-03** — política cableada en `ci.yml` y documentada en
  `docs/verification.md`.

## Trazabilidad

Tests `test_ac_0079_<nn>_*` con marcador `@spec AC-0079-NN`; verificado por
`cargo xtask trace`/`contract`.
