---
id: SPEC-0046
feature: ci_matrix_optimization
status: accepted
owner: platform-team
appetite_days: 3
boundaries:
  crates: []
  out_of_scope: [codigo de producto, ARM, self-hosted]
fr:
  - { id: FR-0046-01, desc: "ci.yml: la matriz del job test se limita a ubuntu+windows (feedback rapido en push/PR)" }
  - { id: FR-0046-02, desc: "nightly.yml: un job test-macos cubre macOS en schedule (cola larga fuera del PR)" }
  - { id: FR-0046-03, desc: "check_ci_config.py valida ubuntu+windows en ci.yml y el job macos en nightly" }
  - { id: FR-0046-04, desc: "docs/verification.md documenta la estrategia de runners" }
nf:
  - { id: NF-0046-01, desc: "T1 por runner sigue < 90 s; PR no espera la cola de macOS" }
  - { id: NF-0046-02, desc: "YAML valido; sin secretos" }
acceptance_criteria:
  - id: AC-0046-01
    given: ci.yml
    when: se valida
    then: la matriz del job test incluye ubuntu-latest y windows-latest y NO exige macos en cada push
    test: test_ac_0046_01_ci_matrix_fast
  - id: AC-0046-02
    given: nightly.yml
    when: se valida
    then: existe un job que ejecuta los tests en macOS (schedule)
    test: test_ac_0046_02_nightly_has_macos
  - id: AC-0046-03
    given: scripts/check_ci_config.py
    when: se ejecuta
    then: valida la matriz rapida y el job macos (exit 0)
    test: test_ac_0046_03_check_ci_config_validates_runners
exit_criteria:
  - python scripts/check_ci_config.py
  - cargo test -p xtask
rollback:
  - revertir los workflows y el script
sandbox:
  - python scripts/check_ci_config.py
---

# SPEC-0046 — Optimización de la matriz de CI (macOS fuera del PR)

## Contexto

La matriz 3-SO encarecía cada push (~20 min por la cola de runners macOS). Se
limita el job `test` de `ci.yml` a **ubuntu + windows** (feedback rápido) y se
mueve macOS a un job en `nightly.yml` (schedule). `check_ci_config` valida ambos.

## Criterios de aceptación

- **AC-0046-01** — matriz rápida en push/PR.
- **AC-0046-02** — macOS en nightly.
- **AC-0046-03** — validación.

## Trazabilidad

Tests en `crates/xtask`; `cargo xtask trace`.
