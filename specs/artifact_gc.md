---
id: SPEC-0192
feature: artifact_gc
status: implemented
maturity: real
owner: devops
appetite_days: 1
boundaries:
  crates: [xtask]
  files:
    - crates/xtask/src/gc.rs
    - crates/xtask/src/main.rs
    - scripts/clean-workspace.ps1
    - .github/workflows/ci.yml
  out_of_scope: [CARGO_HOME, sccache, cache de red, corpus de fuzz]
fr:
  - { id: FR-0192-01, desc: "cargo xtask gc recolecta target/, mutants.out*, fuzz/target, coverage y crates/*/target" }
  - { id: FR-0192-02, desc: "dry-run por defecto; --apply borra solo la allowlist" }
  - { id: FR-0192-03, desc: "politica de antiguedad (24 h por defecto) y --all para ignorarla" }
  - { id: FR-0192-04, desc: "reporte humano y JSON con bytes liberados, antiguedad y accion" }
nf:
  - { id: NF-0192-01, desc: "nunca toca fuente, specs, docs, .git ni la raiz del workspace" }
  - { id: NF-0192-02, desc: "idempotente, sin panics y con errores accionables por candidato" }
  - { id: NF-0192-03, desc: "O(N) stat sobre artefactos candidatos; memoria O(k)" }
acceptance_criteria:
  - id: AC-0192-01
    given: un workspace con target/ y --dry-run
    when: se ejecuta cargo xtask gc
    then: reporta los bytes y no borra nada
    test: test_ac_0192_01_dry_run
  - id: AC-0192-02
    given: un workspace con target/, mutants.out, specs/, docs/ y .git/
    when: se ejecuta cargo xtask gc --apply --all
    then: borra solo los artefactos y conserva fuente, specs, docs y .git
    test: test_ac_0192_02_apply_removes_only_artifacts
  - id: AC-0192-03
    given: un artefacto recien creado
    when: se ejecuta cargo xtask gc --apply sin --all
    then: se conserva por antiguedad; con --all se borra
    test: test_ac_0192_03_age_policy
  - id: AC-0192-04
    given: una recoleccion ya aplicada
    when: se repite y se serializa el reporte
    then: removed_bytes es 0 y el JSON es determinista
    test: test_ac_0192_04_idempotent_json
  - id: AC-0192-05
    given: rutas de fuente, specs, docs, .git y traversal
    when: se evalua la allowlist
    then: ninguna es aceptada como artefacto
    test: test_ac_0192_05_protected_paths
exit_criteria:
  - cargo test -p xtask
  - cargo clippy -p xtask --all-targets -- -D warnings
  - cargo fmt -p xtask -- --check
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - revertir el commit; el recolector solo borra rutas regenerables y no deja estado persistente
sandbox:
  - cargo test -p xtask gc::tests
  - cargo xtask gc --root <tempdir> --dry-run --json
---

# SPEC-0192 — Recolector de artefactos regenerables

## Contexto

Un workspace Rust acumula `target/` (perfiles, incremental, doc, criterio de
benchmarks), `mutants.out/` (cargo-mutants) y perfiles de cobertura. Son
**regenerables** tras un build/test y en CI ocupan GB. El script histórico
`scripts/clean-workspace.ps1` borraba todo sin dry-run, sin política de
antigüedad y solo en Windows. Esta spec define un recolector seguro, portable y
observable dentro de `xtask` (sin runtime nuevo).

## Superficie

- `crates/xtask/src/gc.rs`: allowlist, planificación, borrado, reporte JSON.
- `crates/xtask/src/main.rs`: subcomando `gc`.
- `scripts/clean-workspace.ps1`: delega en `cargo xtask gc --apply --all`.
- `.github/workflows/ci.yml`: paso `if: always()` tras tests/mutación.

Fuera de alcance: `CARGO_HOME` (lo cubre `cargo-cache`), caché de `sccache`,
corpus de `fuzz/artifacts` y cualquier ruta que no sea de la allowlist.

## Diseño

Allowlist explícita `A = {target, mutants.out, mutants.out.old, fuzz/target,
coverage, crates/*/target, *.profraw, *.profdata, *.lcov}`. Para cada candidato
`p ∈ A` existente:

```text
b(p)  = tamaño recursivo (sin seguir symlinks)
m(p)  = max mtime del árbol
age(p)= now - m(p)
borrar(p) ⇔ apply ∧ (all ∨ age(p) ≥ τ)
```

Invariantes de seguridad:

1. `borrar(p) ⇒ p ∈ A ∧ p ⊂ root ∧ p ≠ root`.
2. Un symlink que resuelve fuera de `root` se ignora.
3. Un error de borrado se reporta y no aborta los demás candidatos.

Complejidad: `O(N)` stat/borrado sobre los ficheros de los artefactos (`N`) y
memoria `O(k)` con `k` candidatos. El script anterior recorría todo el árbol
(`O(N)` pero sobre fuente incluida) y materializaba la lista de directorios.

## Estado del arte

- `cargo clean` (oficial): borra todo `target/`; sin antigüedad ni allowlist.
- `cargo-sweep`: borrado selectivo por tiempo/tamaño/toolchain; **unmaintained**
  y dependencia externa.
- `cargo-cache`: gestión de `CARGO_HOME` y `--autoclean` en CI; no cubre
  `target/` del workspace ni `mutants.out`.
- `cargo-clean-all`: borra todos los `target/` de un árbol; sin dry-run/JSON.

## Referencias

- Cargo Book, *cargo-clean*: https://doc.rust-lang.org/cargo/commands/cargo-clean.html
- Cargo Book, *Build cache*: https://doc.rust-lang.org/cargo/reference/build-cache.html
- cargo-sweep (unmaintained): https://github.com/holmgr/cargo-sweep
- cargo-cache: https://github.com/matthiaskrgr/cargo-cache
