---
id: SPEC-0094
feature: red_first_gate
status: implemented
owner: quality-team
appetite_days: 2
boundaries:
  crates: [xtask]
  files: [crates/xtask/src/red.rs, crates/xtask/src/main.rs, crates/xtask/tests/red_first.rs, scripts/red_first_gate.py, scripts/check_ci_config.py, .github/workflows/ci.yml, docs/verification.md]
  out_of_scope: [ejecutar el gate sobre specs implemented, reescribir la historia git para reproducir RED, mutacion por spec (ya en T2), LLM-judge]
fr:
  - { id: FR-0094-01, desc: "cargo xtask red <spec> parsea estado/tests/crates del spec, exige status planned|draft y clasifica la salida de cargo test en RED/pass/missing" }
  - { id: FR-0094-02, desc: "scripts/red_first_gate.py descubre los specs en curso del diff del PR (o --specs/--list) y ejecuta xtask red por cada uno" }
nf:
  - { id: NF-0094-01, desc: "clasificacion determinista y testeable (funcion pura evaluate); sin dependencias externas (xtask solo std); acota cargo test a los crates del spec (-p) cuando estan declarados" }
  - { id: NF-0094-02, desc: "solo PR y solo specs en curso: un spec implemented no es objeto del gate (exit 2 ruidoso si se invoca)" }
acceptance_criteria:
  - id: AC-0094-01
    given: salidas sinteticas de cargo test (error de compilacion, test fallido, todos pasan, cero tests)
    when: se llama red::evaluate
    then: devuelve Red en los dos primeros casos, AlreadyPassing en el tercero y Missing en el cuarto
    test: test_ac_0094_01_evaluate_classifies_red_pass_and_missing
  - id: AC-0094-02
    given: ci.yml y scripts/check_ci_config.py
    when: se inspeccionan
    then: ci.yml declara el job red-first (solo PR) que invoca red_first_gate.py --diff y el guard lo valida
    test: test_ac_0094_02_red_first_job_wired_in_ci_and_guard
  - id: AC-0094-03
    given: un spec implemented y un path inexistente
    when: se ejecuta cargo xtask red con cada uno
    then: ambos salen con exit 2 y el primero explica que solo aplica a specs planned/draft
    test: test_ac_0094_03_red_rejects_implemented_and_missing
  - id: AC-0094-04
    given: un spec planned y otro implemented en ficheros temporales
    when: se ejecuta red_first_gate.py --list --specs con ambos
    then: exit 0 y el listado incluye solo el planned
    test: test_ac_0094_04_gate_lists_only_in_progress_specs
exit_criteria:
  - cargo test -p xtask
  - cargo xtask red specs/semantic_cache_persistence.md  (rechazo ruidoso: implemented)
  - python scripts/red_first_gate.py --diff pr.diff  (no-op si no hay specs en curso)
  - python scripts/check_ci_config.py
rollback:
  - retirar el comando red, scripts/red_first_gate.py, el job red-first y check_red_first_job
sandbox:
  - cargo test -p xtask --test red_first
---

# SPEC-0094 — Gate RED-first (evidencia test-first para specs en curso)

## Contexto

Análisis por flujos 2026-10-13 (roadmap §Estado, «Líneas futuras», P0): el ciclo
SDD exige tests RED antes de implementar (roadmap §2.1), pero no había ningún
harness que lo verificara. La frontera de *adversarial guardrails* 2026 pide una
puerta ejecutable: un spec en curso (`planned`/`draft`) debe tener sus tests
**fallando** hasta que se implemente.

Esta spec añade `cargo xtask red <spec>` (clasificación determinista de la
salida de `cargo test`) y un job de PR `red-first` que descubre los specs en
curso del diff y exige RED en cada uno. Los specs `implemented` quedan fuera:
sus tests deben pasar (trazabilidad `xtask trace`/`contract`).

## Superficie

- `crates/xtask/src/red.rs`: parser (`parse_spec`) y clasificador puro
  (`evaluate` → `Red | AlreadyPassing | Missing`).
- `crates/xtask/src/main.rs`: comando `red`.
- `scripts/red_first_gate.py`: descubre specs del diff (`--diff`), lista
  (`--list`) o recibe `--specs`; ejecuta `cargo xtask red` por cada spec en curso.
- `.github/workflows/ci.yml`: job `red-first` (solo PR).
- `scripts/check_ci_config.py`: `check_red_first_job`.
- Tests: `crates/xtask/tests/red_first.rs` + unitarios de `red.rs`.

## Criterios de aceptación

- **AC-0094-01** — clasificación RED/pass/missing determinista.
- **AC-0094-02** — job `red-first` cableado y validado.
- **AC-0094-03** — `xtask red` rechaza `implemented` y paths ausentes (exit 2).
- **AC-0094-04** — el script lista solo specs en curso.

## Trazabilidad

Tests `test_ac_0094_<nn>_*` con marcador `@spec AC-0094-NN`; verificado por
`cargo xtask trace`/`contract`.
