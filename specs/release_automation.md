---
id: SPEC-0089
feature: release_automation
status: implemented
owner: release-team
appetite_days: 2
boundaries:
  crates: [xtask]
  files: [scripts/check_commits.py, scripts/check_changelog.py, .github/workflows/release.yml, .github/workflows/ci.yml, scripts/check_ci_config.py, docs/verification.md, CHANGELOG.md]
  out_of_scope: [publicacion en crates.io/registries, firma de artefactos/attestations, changelog autogenerado desde git log, releases de bindings por lenguaje, auto-merge de Dependabot]
fr:
  - { id: FR-0089-01, desc: "scripts/check_commits.py valida Conventional Commits en el rango de un PR (stdlib, sin dependencias)" }
  - { id: FR-0089-02, desc: "scripts/check_changelog.py exige una seccion versionada en CHANGELOG.md para el tag que se libera y un [Unreleased] con contenido" }
  - { id: FR-0089-03, desc: "release.yml publica un GitHub Release al empujar un tag v* (notas del CHANGELOG + SBOM); ci.yml gana un job commits (solo PR) que corre ambos validadores" }
nf:
  - { id: NF-0089-01, desc: "solo stdlib de Python (consistente con SPEC-0061/SPEC-0079); sin dependencias nuevas" }
  - { id: NF-0089-02, desc: "minimo privilegio: ci.yml con contents: read; release.yml con contents: write solo en el workflow de tag" }
  - { id: NF-0089-03, desc: "contrato de fallo ruidoso: rango vacio o CHANGELOG ausente => exit 2, nunca verde silencioso" }
acceptance_criteria:
  - id: AC-0089-01
    given: un rango con mensajes Conventional Commits validos y otro con un mensaje invalido
    when: se ejecuta check_commits.py sobre cada rango
    then: exit 0 en el valido y exit 1 nombrando el commit infractor en el invalido
    test: test_ac_0089_01_commit_lint_accepts_and_rejects
  - id: AC-0089-02
    given: el CHANGELOG.md y un tag vX.Y.Z
    when: se ejecuta check_changelog.py con ese tag
    then: exit 0 si existe [X.Y.Z] con contenido; exit 1 si falta; exit 2 si el fichero no existe
    test: test_ac_0089_02_changelog_gate_enforces_version
  - id: AC-0089-03
    given: ci.yml y .github/workflows/release.yml
    when: se inspeccionan
    then: ci.yml declara el job commits (solo PR) que invoca ambos scripts y release.yml dispara en tags v* con contents: write y adjunta el SBOM
    test: test_ac_0089_03_release_wiring_in_ci_and_workflow
exit_criteria:
  - cargo test -p xtask --test release_automation
  - python scripts/check_commits.py --message "feat(db): example"  (run local real)
  - python scripts/check_changelog.py CHANGELOG.md --tag v0.1.0
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - python scripts/check_ci_config.py
rollback:
  - retirar release.yml, el job commits y los dos scripts; ci.yml vuelve a su forma anterior
sandbox:
  - cargo test -p xtask --test release_automation
---

# SPEC-0089 — Automatizacion de releases/tags y validacion de CHANGELOG/commits (Ola 5)

## Contexto

Analisis por flujos de trabajo 2026-10-13 (roadmap §Estado «Lineas futuras»,
P0): el repositorio **no tiene ningun tag ni release publicado** y las tags
`v*` no disparan nada. La disciplina de Conventional Commits y el CHANGELOG se
mantienen a mano, sin verificacion en CI (un PR puede entrar con un mensaje
`WIP` o sin entrada en el CHANGELOG). Riesgo: releases inconsistentes y trazado
de cambios roto justo cuando el proyecto pasa a produccion.

Frontera 2026: `git-cliff`/`release-please` automatizan changelog+tag a partir
de Conventional Commits y auditan el rango del PR; el patron de minimo
privilegio (`contents: read` por defecto, `contents: write` solo en el publish)
es el recomendado por la guia de hardening de GitHub Actions.

## Superficie

- `scripts/check_commits.py`: valida `type(scope): descripcion` en un rango de
  git (`--range`), en un fichero de mensajes (`--file`) o en uno suelto
  (`--message`). Ignora merge/revert de git; fallo ruidoso (exit 2) si no hay
  commits que revisar.
- `scripts/check_changelog.py`: exige `[Unreleased]` con contenido y, con
  `--tag vX.Y.Z`, la seccion `[X.Y.Z]` con contenido.
- `.github/workflows/ci.yml` (job `commits`, solo PR): corre ambos scripts
  contra el rango `origin/<base>..HEAD`.
- `.github/workflows/release.yml`: en `push: tags: v*` (y `workflow_dispatch`)
  corre los gates T1 y publica un GitHub Release con las notas extraidas del
  CHANGELOG y el SBOM CycloneDX como artefacto.
- `scripts/check_ci_config.py`: valida el cableado (job `commits` + release.yml).
- Tests: `crates/xtask/tests/release_automation.rs`.

## Criterios de aceptacion

- **AC-0089-01** — el linter de commits acepta Conventional Commits validos
  (exit 0) y rechaza invalidos nombrando el infractor (exit 1); sin entrada,
  exit 2.
- **AC-0089-02** — el gate de CHANGELOG exige la seccion de la version del tag
  (exit 0/1) y falla ruidoso si el fichero no existe (exit 2).
- **AC-0089-03** — cableado real: job `commits` (solo PR) en `ci.yml` y
  `release.yml` con `tags: v*`, `contents: write` y SBOM.

## Trazabilidad

Tests `test_ac_0089_<nn>_*` con marcador `@spec AC-0089-NN`; verificado por
`cargo xtask trace`/`contract`.
