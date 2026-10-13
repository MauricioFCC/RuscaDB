---
id: SPEC-0082
feature: slo_gates
status: implemented
owner: quality-team
appetite_days: 3
boundaries:
  crates: [ruscadb, xtask]
  files: [scripts/slo_gate.py, .github/workflows/slo.yml, docs/verification.md, benches/baselines/*, crates/ruscadb/tests/slo_gates.rs]
  out_of_scope: [publicación de dashboards, gates en nightly (alert-only ya existe), medición en runners self-hosted, gate de memoria/energía]
fr:
  - { id: FR-0082-01, desc: "gate de latencia con Mann-Whitney U (p < 0.002, direccional) comparando candidato vs baseline committed, en vez de ratios de umbral fijo" }
  - { id: FR-0082-02, desc: "gate de recall@10 con bootstrap pareado (10k remuestreos, CI 99%) sobre los mismos pares de consulta, sin gatear ratios absolutos" }
  - { id: FR-0082-03, desc: "calibración A/A: dos muestras de la misma build pasan el gate (noise floor sin falsos positivos con el CV de los runners ≈ 2.7 %)" }
  - { id: FR-0082-04, desc: "workflow de CI ejecuta el gate en PRs sobre los artefactos criterion/baselines committed" }
nf:
  - { id: NF-0082-01, desc: "solo stdlib de Python (consistente con SPEC-0079); sin red en el gate" }
  - { id: NF-0082-02, desc: "fallo ruidoso: fixtures malformados o baselines ausentes => exit 2, nunca verde silencioso" }
  - { id: NF-0082-03, desc: "documentado en docs/verification.md con cómo correrlo local" }
acceptance_criteria:
  - id: AC-0082-01
    given: fixture de baseline y candidato con latencia claramente peor
    when: corre el gate Mann-Whitney
    then: exit 1 nombrando la métrica regresionada; con muestras iguales => exit 0
    test: test_ac_0082_01_mann_whitney_flags_significant_regression
  - id: AC-0082-02
    given: fixture de recall emparejado con delta significativo y otro sin delta
    when: corre el bootstrap pareado
    then: falla el primero (p < 0.002) y pasa el segundo (p >= 0.002)
    test: test_ac_0082_02_bootstrap_paired_recall_gates_delta
  - id: AC-0082-03
    given: fixture A/A (misma distribución, dos corridas)
    when: corre el gate completo
    then: exit 0 (calibrado, sin falso positivo) y un fixture malformado => exit 2
    test: test_ac_0082_03_aa_calibration_and_loud_failure
exit_criteria:
  - cargo test -p ruscadb --test slo_gates
  - python scripts/slo_gate.py --help  (run local)
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - python scripts/check_ci_config.py
  - cargo xtask trace / contract verdes
rollback:
  - retirar slo_gate.py y slo.yml (vuelven los gates de umbral fijo)
sandbox:
  - cargo test -p ruscadb --test slo_gates
---

# SPEC-0082 — Gates estadísticos SLO (Ola 3)

## Contexto

La auditoría y el research T6 (§3): los runners de GitHub tienen CV ≈ 2.7 %
— gatear «latencia ≤ baseline × 1.10» o «recall ≥ 0.93» produce falsos
positivos por ruido y falsos negativos por tolerancias mal calibradas. La
frontera (ASV, ASE'26) sustituye umbrales por **tests de hipótesis**:
Mann-Whitney U con p < 0.002 (familia de métricas ≈ 25 → Bonferroni
aproximado) para latencias, y **bootstrap pareado** sobre los mismos pares
de consulta para recall, más una corrida **A/A** que calibra el noise floor.

## Superficie

- `scripts/slo_gate.py` (nuevo, solo stdlib): lee fixtures JSON
  (baseline committed en `benches/baselines/` + candidato de la corrida),
  aplica Mann-Whitney direccional y bootstrap pareado 10k con semilla fija,
  exit 0/1/2 (contrato ruidoso SPEC-0079).
- `.github/workflows/slo.yml` (nuevo, PR): corre benches rápidos, genera el
  JSON del candidato y ejecuta el gate. No toca `ci.yml`.
- `crates/ruscadb/tests/slo_gates.rs` (nuevo): invoca el script sobre
  fixtures versionados (patrón SPEC-0079, intérprete `python`/`python3`).
- `docs/verification.md`: sección «Gates estadísticos SLO».

## Criterios de aceptación

- **AC-0082-01** — Mann-Whitney detecta regresión significativa (p < 0.002)
  y no falla con muestras iguales.
- **AC-0082-02** — bootstrap pareado gatea delta de recall por significancia.
- **AC-0082-03** — A/A pasa; fixtures malformados => exit 2.

## Trazabilidad

Tests `test_ac_0082_<nn>_*` con marcador `@spec AC-0082-NN`; verificado por
`cargo xtask trace`/`contract`.
