---
id: SPEC-0098
feature: mutation_gate_schema
status: implemented
owner: quality-team
appetite_days: 1
boundaries:
  crates: [xtask]
  files: [scripts/mutation_diff_gate.py, crates/xtask/tests/mutation_gate.rs, crates/xtask/tests/fixtures/mutation/]
  out_of_scope: [cambiar el umbral (70), instalar cargo-mutants, mutacion in-place]
fr:
  - { id: FR-0098-01, desc: "mutation_diff_gate.py puntua tanto el esquema dict legacy como el de cadena de cargo-mutants v25+ (CaughtMutant/Timeout/MissedMutant/Unviable)" }
  - { id: FR-0098-02, desc: "un summary de estado desconocido sigue siendo fallo ruidoso (exit 2), nunca PASS silencioso" }
nf:
  - { id: NF-0098-01, desc: "solo stdlib; el esquema tolerante evita que una version nueva de cargo-mutants convierta el gate en un no-op" }
  - { id: NF-0098-02, desc: "timeout cuenta como matado (politica SPEC-0061) en ambos esquemas" }
acceptance_criteria:
  - id: AC-0098-01
    given: un outcomes.json con summary de cadena (2 caught, 1 timeout, 1 missed, 1 unviable)
    when: se ejecuta el gate con umbral 70 y con umbral 80
    then: reporta MS_diff=75.0, exit 0 con 70 y exit 1 con 80
    test: test_ac_0098_01_string_summary_schema_is_scored
  - id: AC-0098-02
    given: un outcomes.json con summary dict legacy (3 caught, 1 missed, 1 unviable)
    when: se ejecuta el gate con umbral 70
    then: reporta MS_diff=75.0 y exit 0 (compatibilidad hacia atras)
    test: test_ac_0098_02_legacy_dict_schema_still_supported
  - id: AC-0098-03
    given: un outcomes.json con un summary de estado desconocido
    when: se ejecuta el gate
    then: exit 2 nombrando el estado no reconocido (fallo ruidoso)
    test: test_ac_0098_03_unknown_summary_fails_loudly
exit_criteria:
  - cargo test -p xtask --test mutation_gate
  - python scripts/mutation_diff_gate.py mutants.out/outcomes.json --min-score 70
rollback:
  - revertir mutation_diff_gate.py al parseo dict-only y retirar los fixtures/tests
sandbox:
  - cargo test -p xtask --test mutation_gate
---

# SPEC-0098 — Robustez de esquema del gate de mutación

## Contexto

Durante la corrida **mutante adversarial** de SPEC-0096 (`cargo mutants
--in-diff`, v27.1.0) se detectó que `scripts/mutation_diff_gate.py` solo
reconocía el esquema *dict* (`summary: {"caught": N}`); cargo-mutants ≥ v25
serializa `summary` como **cadena** (`"CaughtMutant"`, `"Timeout"`, ...). Con el
esquema nuevo el gate contaba 0 mutantes y devolvía **PASS silencioso**
(`sin mutantes puntuables`), desactivando de facto la puerta T2 en CI (que
instala la última versión de cargo-mutants).

Corregido: el gate clasifica ambos esquemas y mantiene el fallo ruidoso ante
estados desconocidos. El caso real de SPEC-0096 puntuó **MS_diff = 100 % (7/7:
4 caught + 3 timeout, 0 missed)**.

## Superficie

- `scripts/mutation_diff_gate.py`: `classify()` (dict legacy + cadena v25+) y
  `score_outcomes()`.
- `crates/xtask/tests/mutation_gate.rs` + fixtures en
  `crates/xtask/tests/fixtures/mutation/`.

## Criterios de aceptación

- **AC-0098-01** — esquema de cadena puntuado (75 %), umbral 70/80.
- **AC-0098-02** — esquema dict legacy sigue puntuado.
- **AC-0098-03** — estado desconocido ⇒ exit 2.

## Trazabilidad

Tests `test_ac_0098_<nn>_*` con marcador `@spec AC-0098-NN`; verificado por
`cargo xtask trace`/`contract`.
