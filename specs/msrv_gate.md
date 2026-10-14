---
id: SPEC-0090
feature: msrv_gate
status: implemented
owner: quality-team
appetite_days: 1
boundaries:
  crates: [xtask]
  files: [.github/workflows/ci.yml, scripts/check_ci_config.py, docs/verification.md, README.md]
  out_of_scope: [subir o bajar el MSRV, builds nightly, cache dedicado para el job MSRV, matriz MSRV x OS]
fr:
  - { id: FR-0090-01, desc: "ci.yml declara un job msrv que compila el workspace con la version de rust-version (1.89) y --locked" }
  - { id: FR-0090-02, desc: "check_ci_config.py valida el job msrv contra la fuente unica de verdad (rust-version del Cargo.toml raiz)" }
nf:
  - { id: NF-0090-01, desc: "la version MSRV vive en un solo lugar (Cargo.toml rust-version); CI la referencia y el guard la compara, sin duplicar el numero" }
  - { id: NF-0090-02, desc: "el job msrv usa cargo check (no linkea) para ser rapido y barato en el PR" }
acceptance_criteria:
  - id: AC-0090-01
    given: .github/workflows/ci.yml
    when: se inspecciona el job msrv
    then: declara el toolchain MSRV (1.89) y ejecuta cargo check --workspace --all-features --locked
    test: test_ac_0090_01_msrv_job_in_ci
  - id: AC-0090-02
    given: el Cargo.toml raiz y ci.yml
    when: se comparan via scripts/check_ci_config.py
    then: la version del job msrv coincide con rust-version del workspace y el guard la valida
    test: test_ac_0090_02_msrv_version_single_source
exit_criteria:
  - cargo test -p xtask --test msrv_gate
  - python scripts/check_ci_config.py
  - cargo check --workspace --all-features --locked  (run local real con 1.89)
rollback:
  - retirar el job msrv de ci.yml y la funcion check_msrv_job del guard
sandbox:
  - cargo test -p xtask --test msrv_gate
---

# SPEC-0090 — Job MSRV en CI (Ola 5)

## Contexto

Analisis por flujos 2026-10-13 (roadmap §Estado «Lineas futuras», P0): el
workspace declara `rust-version = "1.89"` (MSRV, justificado por
`std::fs::File::try_lock`, SPEC-0076) y lo publica en el README/CONTRIBUTING,
pero **ningun job compila con 1.89**. La toolchain de build esta pinneada en
`rust-toolchain.toml` (1.97.1) y CI usa `stable`; el MSRV es por tanto una
promesa no verificada — el dia que una dependencia suba su propio MSRV, se
rompera sin que el CI lo detecte (cargo no valida el `rust-version` de las
dependencias por defecto).

## Superficie

- `.github/workflows/ci.yml`: nuevo job `msrv` (T1) con
  `dtolnay/rust-toolchain@<MSRV>` y `cargo check --workspace --all-features
  --locked`.
- `scripts/check_ci_config.py`: `check_msrv_job` lee `rust-version` del
  `Cargo.toml` raiz (fuente unica) y exige que el job `msrv` exista y cite esa
  version; tambien exige `cargo check` y `--locked` dentro del job.
- Tests: `crates/xtask/tests/msrv_gate.rs`.

## Criterios de aceptacion

- **AC-0090-01** — el job `msrv` existe, usa el toolchain 1.89 y corre
  `cargo check --workspace --all-features --locked`.
- **AC-0090-02** — la version del job coincide con `rust-version` del workspace
  y `check_ci_config.py` lo valida (sin duplicar el numero en el guard).

## Trazabilidad

Tests `test_ac_0090_<nn>_*` con marcador `@spec AC-0090-NN`; verificado por
`cargo xtask trace`/`contract`.
