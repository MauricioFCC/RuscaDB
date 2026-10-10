---
id: SPEC-0061
feature: t2_judge_gate
status: accepted
owner: qa-team
appetite_days: 5
boundaries:
  crates: [xtask]
  out_of_scope: [backend LLM en CI, juicio semantico de specs, reescribir T2]
fr:
  - { id: FR-0061-01, desc: "cargo xtask contract: todo test_ac_* numerico lleva marcador @spec y todo marcador resuelve a un AC existente" }
  - { id: FR-0061-02, desc: "toda spec implemented tiene sus AC trazados (consistencia de estado)" }
  - { id: FR-0061-03, desc: "cargo xtask judge: rubricas deterministicas sobre el diff (unwrap, markers, unsafe) con veredicto pass/warn/fail" }
  - { id: FR-0061-04, desc: "kappa de Cohen + tasas false-pass/false-fail sobre juicios versionados (humano vs juez)" }
nf:
  - { id: NF-0061-01, desc: "cero falsos-fail: las rubricas heuriticas solo avisan (warn), nunca fallan" }
  - { id: NF-0061-02, desc: "solo std, sin dependencias nuevas, < 5 s en local" }
acceptance_criteria:
  - id: AC-0061-01
    given: el workspace actual
    when: se ejecuta cargo xtask contract
    then: todo fn test_ac_XXXX_NN_ tiene marcador @spec AC-XXXX-NN cercano
    test: test_ac_0061_01_contract_markers_bidirectional
  - id: AC-0061-02
    given: las specs con status implemented
    when: se ejecuta el contrato
    then: todos sus AC estan trazados a tests existentes
    test: test_ac_0061_02_implemented_specs_fully_traced
  - id: AC-0061-03
    given: un diff fixture con violaciones plantadas y otro limpio
    when: se evaluan las rubricas
    then: el sucio da FAIL con los ids correctos y el limpio SUCCESS
    test: test_ac_0061_03_judge_rubrics_on_fixture_diff
  - id: AC-0061-04
    given: una matriz de acuerdo 2x2 conocida
    when: se calcula kappa y las tasas
    then: coinciden con los valores esperados (epsilon 1e-9)
    test: test_ac_0061_04_kappa_fixture
exit_criteria:
  - cargo test -p xtask -- test_ac_0061
  - cargo xtask contract verde en el workspace
  - cargo xtask trace intacto (solo fallan las aceptadas pendientes)
rollback:
  - revertir xtask (herramienta, sin efecto en producto)
sandbox:
  - cargo test -p xtask
---

# SPEC-0061 — Puerta T2: contrato de trazabilidad + juez determinista

## Contexto

La auditoría demostró que T2 no existe (sin spec del juez, sin gate de
mutación en merge, sin contrato RED). Este es el primer tramo real: trazabilidad
bidireccional máquina-verificable (`contract`) + juez determinista v0 sobre el
diff (`judge`) con rúbricas precisas y veredicto. El backend LLM queda
documentado como futuro (mismo patrón que ONNX: declarado, no cableado); lo
que sí se implementa hoy es el marco de validación del juez (κ + false-pass /
false-fail, curso AI Evals W2) para cuando exista.

## Criterios de aceptación

- **AC-0061-01/02** — marcadores y consistencia de estado.
- **AC-0061-03/04** — rúbricas sobre fixtures y κ correcto.

## Trazabilidad

Tests `test_ac_0061_<nn>_*`; verificado por `cargo xtask trace`.
