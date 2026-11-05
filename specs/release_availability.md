---
id: SPEC-0208
feature: release_availability
status: implemented
maturity: real
owner: release-ops
appetite_days: 3
boundaries:
  crates: [xtask]
  files:
    - scripts/install.sh
    - scripts/install.ps1
    - scripts/check_release_assets.py
    - .github/workflows/release.yml
    - README.md
    - docs/instalacion.md
  out_of_scope: [publicar crates.io/PyPI/npm, firma Sigstore, mirrors]
fr:
  - { id: FR-0208-01, desc: "matriz unica target->asset validada offline (sin red)" }
  - { id: FR-0208-02, desc: "instaladores usan releases/latest/download y fallan con fallback git accionable si 404" }
  - { id: FR-0208-03, desc: "release.yml en tags v* publica assets ruscadb-<target>.tar.gz y .sha256" }
  - { id: FR-0208-04, desc: "README/instalacion documentan cargo install --git ... --locked y --dry-run" }
nf:
  - { id: NF-0208-01, desc: "el gate offline no hace red; O(targets) y exit 0/1/2" }
  - { id: NF-0208-02, desc: "sin secretos; owner/repo configurable por env/parametro" }
acceptance_criteria:
  - id: AC-0208-01
    given: la matriz de targets del release
    when: se ejecuta scripts/check_release_assets.py --dry-run
    then: valida nombres de asset/URL base y sale 0 sin red
    test: test_ac_0208_01_release_assets_matrix
  - id: AC-0208-02
    given: un repositorio sin release publicado (404)
    when: se ejecuta install.sh/install.ps1
    then: falla con mensaje WHAT+WHY+WHERE y sugiere cargo install --git
    test: test_ac_0208_02_install_fallback
  - id: AC-0208-03
    given: un tag vX.Y.Z
    when: corre release.yml
    then: publica ruscadb-<target>.tar.gz + .sha256 para cada target
    test: test_ac_0208_03_release_workflow
  - id: AC-0208-04
    given: un usuario sin release
    when: lee README/instalacion
    then: encuentra el fallback git y `cargo install ... --dry-run`
    test: test_ac_0208_04_install_dry_run_docs
exit_criteria:
  - python scripts/check_release_assets.py --dry-run
  - cargo install --git https://github.com/MauricioFCC/RuscaDB --package ruscadb-cli --locked --dry-run
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar el validador y la seccion de docs; los instaladores vuelven a su version previa
sandbox:
  - python scripts/check_release_assets.py --dry-run --json
---
# SPEC-0208 — Disponibilidad de release e instalación

## Contexto

`releases/latest` devuelve 404 porque no hay tag `v*` publicado. Los
instaladores resolvían la versión desde esa API y fallaban de forma genérica;
`release.yml` ya define la matriz de binarios pero no había validador offline.

## Diseño

1. `scripts/check_release_assets.py --dry-run` valida matriz target→asset,
   workflow, instaladores y docs sin red.
2. Instaladores: 404 ⇒ error accionable con
   `cargo install --git https://github.com/MauricioFCC/RuscaDB --package ruscadb-cli --locked`.
3. `release.yml`: tags `v*` construyen la matriz y publican `.tar.gz` + `.sha256`.
4. README/`docs/instalacion.md`: estado honesto, fallback git y `--dry-run`.

## Complejidad

```text
gate: O(T) targets, O(1) memoria, sin red
instalación: O(1) requests; fallback git compila localmente
```

## TDD adversarial

Matriz offline; scripts con fallback; workflow con assets/checksums; docs con
`--dry-run` y 404.

## Riesgos

El tag `v*` debe crearse/pushearse para publicar el release (acción operativa
fuera de esta sesión); el fallback git es la vía soportada mientras tanto.
