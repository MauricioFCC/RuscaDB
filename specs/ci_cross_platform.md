---
id: SPEC-0033
feature: ci_cross_platform
status: accepted
owner: platform-team
appetite_days: 3
boundaries:
  crates: []
  out_of_scope: [codigo de producto, releases, ARM]
fr:
  - { id: FR-0033-01, desc: "ci.yml: job test con matriz OS (ubuntu/windows/macos)" }
  - { id: FR-0033-02, desc: "nightly.yml: job sanitizers (ASan) sobre la suite FFI" }
  - { id: FR-0033-03, desc: "check_ci_config.py valida la matriz y el job sanitizers" }
  - { id: FR-0033-04, desc: "docs/verification.md documenta los gates cross-platform y ASan" }
nf:
  - { id: NF-0033-01, desc: "T1 sigue < 90 s por runner; la matriz corre en paralelo" }
  - { id: NF-0033-02, desc: "YAML valido y sin secretos" }
acceptance_criteria:
  - id: AC-0033-01
    given: ci.yml
    when: se valida
    then: el job test declara una matriz con ubuntu-latest, windows-latest y macos-latest
    test: test_ac_0033_01_ci_has_os_matrix
  - id: AC-0033-02
    given: nightly.yml
    when: se valida
    then: contiene un job sanitizers (ASan) alert-only
    test: test_ac_0033_02_nightly_has_sanitizers
  - id: AC-0033-03
    given: scripts/check_ci_config.py
    when: se ejecuta
    then: valida la matriz OS y el job sanitizers (exit 0 con el repo actual)
    test: test_ac_0033_03_check_ci_config_validates_matrix
exit_criteria:
  - python scripts/check_ci_config.py
  - cargo test -p xtask
rollback:
  - revertir los workflows y el script
sandbox:
  - python scripts/check_ci_config.py
---

# SPEC-0033 — CI cross-platform + sanitizers

## Contexto

F6 exige cross-platform y F5 "0 UB ASan/UBSan". Se añade:

- `ci.yml`: job `test` con `strategy.matrix.os: [ubuntu-latest, windows-latest,
  macos-latest]`.
- `nightly.yml`: job `sanitizers` (ASan sobre `cargo test -p ruscadb-ffi
  --target x86_64-unknown-linux-gnu` con `RUSTFLAGS=-Zsanitizer=address`),
  `continue-on-error: true`.
- `scripts/check_ci_config.py`: valida ambos.

## Criterios de aceptación

- **AC-0033-01/02** — matriz OS y job sanitizers.
- **AC-0033-03** — validación en el script.

## Trazabilidad

Tests en `crates/xtask`; `cargo xtask trace`.
