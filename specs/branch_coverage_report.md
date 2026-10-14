---
id: SPEC-0092
feature: branch_coverage_report
status: implemented
owner: quality-team
appetite_days: 1
boundaries:
  crates: [xtask]
  files: [scripts/branch_coverage_report.py, scripts/check_ci_config.py, .github/workflows/ci.yml, docs/verification.md]
  out_of_scope: [re-ejecutar la suite instrumentada (reutiliza el lcov del gate), subir/bajar umbrales, gate de lineas (ya en coverage_gate.py), cobertura por fichero]
fr:
  - { id: FR-0092-01, desc: "scripts/branch_coverage_report.py reporta la cobertura BRDA por scope y crate, aplica --min-branch y vuelca --json-out (stdlib, reutiliza el parser de coverage_gate.py)" }
  - { id: FR-0092-02, desc: "el job coverage de ci.yml publica el reporte como artefacto y scripts/check_ci_config.py valida el cableado" }
nf:
  - { id: NF-0092-01, desc: "solo stdlib; no duplica el parseo de lcov (importa coverage_gate.py) ni el run instrumentado (reutiliza lcov.info)" }
  - { id: NF-0092-02, desc: "fallo ruidoso: lcov inexistente/vacio o scope sin ficheros => exit 2, nunca verde silencioso" }
acceptance_criteria:
  - id: AC-0092-01
    given: un lcov con dos scopes (beta 50 % de ramas, alpha 100 %) y una ruta de salida JSON
    when: se ejecuta el reporte con --min-branch 70 para beta y 10 para alpha
    then: exit 1 citando el scope infractor; al relajar beta a 10 sale 0 y el JSON desglosa el % de rama por crate
    test: test_ac_0092_01_branch_report_gates_and_emits_json
  - id: AC-0092-02
    given: ci.yml y scripts/check_ci_config.py
    when: se inspeccionan
    then: el job coverage invoca branch_coverage_report.py con --json-out y sube el artefacto branch-coverage-report; el guard lo valida
    test: test_ac_0092_02_report_wired_in_ci_and_guard
exit_criteria:
  - cargo test -p xtask --test branch_coverage
  - python scripts/branch_coverage_report.py lcov.info --scope ruscadb-core/ --min-branch 70
  - python scripts/check_ci_config.py
rollback:
  - retirar scripts/branch_coverage_report.py, el step/artefacto del job coverage y check_branch_report_job
sandbox:
  - cargo test -p xtask --test branch_coverage
---

# SPEC-0092 — Reporte explícito de cobertura de ramas (BRDA) en CI

## Contexto

Análisis por flujos 2026-10-13 (roadmap §Estado, «Líneas futuras», P0): el
gate de cobertura ya medía ramas con `coverage_gate.py --min-branch`, pero no
existía un **reporte explícito** que citara la cobertura BRDA por crate ni un
artefacto auditable por release. La frontera 2026 (DORA / Google SRE) recomienda
que la cobertura sea *observable* (reporte adjunto), no solo un umbral que pasa
o falla en silencio.

Esta spec cierra ese P0 con un reporte que **reutiliza** el `lcov.info` del job
`coverage` (sin duplicar la corrida instrumentada) y el parser de
`coverage_gate.py` (sin duplicar lcov).

## Superficie

- `scripts/branch_coverage_report.py`: reporta rama por scope y crate, aplica
  `--min-branch`, `--json-out` (artefacto) y falla ruidoso (exit 2).
- `.github/workflows/ci.yml` (job `coverage`): step del reporte + artefacto
  `branch-coverage-report`.
- `scripts/check_ci_config.py`: `check_branch_report_job` valida el cableado.
- Tests: `crates/xtask/tests/branch_coverage.rs`.

## Criterios de aceptación

- **AC-0092-01** — el reporte respeta los umbrales (exit 1/0) y vuelca el
  desglose por crate a JSON.
- **AC-0092-02** — cableado real en `ci.yml` y validado por el guard.

## Trazabilidad

Tests `test_ac_0092_<nn>_*` con marcador `@spec AC-0092-NN`; verificado por
`cargo xtask trace`/`contract`.
