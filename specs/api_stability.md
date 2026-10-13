---
id: SPEC-0078
feature: api_stability
status: implemented
owner: api-team
appetite_days: 2
boundaries:
  crates: [ruscadb-core, ruscadb, ruscadb-ffi]
  files: [.github/workflows/semver.yml, crates/ruscadb/tests/api_stability.rs, crates/ruscadb-ffi/tests/version_abi.rs]
  out_of_scope: ["#[non_exhaustive] en ScalarValue/IR (ripple masivo en el query engine, se difiere)", publicación en crates.io, política de MSRV floor, deprecations activas]
fr:
  - { id: FR-0078-01, desc: "RuscaError es #[non_exhaustive]: añadir variantes futuras no rompe la compilación de consumidores externos" }
  - { id: FR-0078-02, desc: "la C-ABI expone RUSCADB_ABI_VERSION (u32) y ruscadb_version() con la semver de la librería" }
  - { id: FR-0078-03, desc: "CI ejecuta cargo-semver-checks en PRs contra una baseline git (rev base), sin publicar en crates.io" }
nf:
  - { id: NF-0078-01, desc: "workflow nuevo semver.yml, no se modifica ci.yml; supera python scripts/check_ci_config.py" }
  - { id: NF-0078-02, desc: "el ripple de non_exhaustive dentro del workspace se resuelve con brazo wildcard + context (sin swallow de errores)" }
  - { id: NF-0078-03, desc: "docstrings ES-UTF8; sin unwrap/expect en producción; errores WHAT+WHY+WHERE" }
acceptance_criteria:
  - id: AC-0078-01
    given: la definición de RuscaError en ruscadb-core
    when: un consumidor externo hace match exhaustivo
    then: el compilador exige brazo `_` porque el enum es #[non_exhaustive] (verificado por aserción sobre el fuente)
    test: test_ac_0078_01_error_enum_is_non_exhaustive
  - id: AC-0078-02
    given: la C-ABI de ruscadb-ffi
    when: se leen RUSCADB_ABI_VERSION y ruscadb_version()
    then: ABI >= 1 y la versión coincide con CARGO_PKG_VERSION del workspace
    test: test_ac_0078_02_abi_version_symbols
  - id: AC-0078-03
    given: .github/workflows/semver.yml
    when: se inspecciona el YAML
    then: existe, corre en pull_request y usa cargo-semver-checks con baseline git (no contra crates.io)
    test: test_ac_0078_03_semver_workflow_wired
exit_criteria:
  - cargo test -p ruscadb --test api_stability
  - cargo test -p ruscadb-ffi
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - python scripts/check_ci_config.py
  - cargo xtask trace / contract verdes
rollback:
  - retirar #[non_exhaustive], los símbolos de versión y semver.yml (la API vuelve a ser "breakable" sin CI)
sandbox:
  - cargo test -p ruscadb --test api_stability
---

# SPEC-0078 — Estabilidad de API y semver (Ola 1 del plan de producción)

## Contexto

Auditoría *production readiness* 2026-10-12 (riesgo 2): **API inestable sin
red de seguridad** — cero `#[non_exhaustive]`, cero `#[deprecated]`, sin
control semver y sin versión/ABI en la C-ABI. Hoy no hay usuarios externos,
así que cada ruptura es gratis; mañana no.

Frontera 2026 (§3, research T2): `cargo-semver-checks` v0.51 (Action v2)
compara rustdoc JSON de HEAD contra una **baseline git** — no hace falta
publicar en crates.io (el workspace es `publish = false`); `#[non_exhaustive]`
es el mecanismo estándar de Rust para crecer sin romper; la C-ABI necesita
`RUSCADB_ABI_VERSION` (invariante ABI) separado de la semver semántica.

## Superficie

- `crates/ruscadb-core/src/error.rs`: `#[non_exhaustive]` sobre
  `pub enum RuscaError` (prep: variantes `DatabaseInUse`/`SchemaVersionTooNew`/
  `MigrationFailed` ya añadidas en Ola 0). El ripple interno (matches sin
  wildcard en otros crates) se resuelve con brazo `_` + contexto.
- `crates/ruscadb-ffi/src/lib.rs`: `pub static RUSCADB_ABI_VERSION: u32` +
  `pub extern "C" fn ruscadb_version()` (devuelve la semver en buffer/longitud,
  según el patrón de `ruscadb_last_error`).
- `.github/workflows/semver.yml` (nuevo): `cargo semver-checks check-release
  --baseline-rev` sobre la rev base del PR. No toca `ci.yml`.
- Tests: `crates/ruscadb/tests/api_stability.rs` (aserciones de fuente) y
  `crates/ruscadb-ffi/tests/version_abi.rs`.

## Criterios de aceptación

- **AC-0078-01** — `RuscaError` es `#[non_exhaustive]` (aserción sobre fuente).
- **AC-0078-02** — símbolos `RUSCADB_ABI_VERSION`/`ruscadb_version()` coherentes
  con `CARGO_PKG_VERSION`.
- **AC-0078-03** — `semver.yml` cableado con baseline git en PRs.

## Trazabilidad

Tests `test_ac_0078_<nn>_*` con marcador `@spec AC-0078-NN`; verificado por
`cargo xtask trace`/`contract`.
