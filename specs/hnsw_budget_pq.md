---
id: SPEC-0058
feature: hnsw_budget_pq
status: implemented
owner: vector-team
appetite_days: 6
boundaries:
  crates: [ruscadb-vector]
  out_of_scope: [IVF en disco, GPU, reentrenamiento de codebooks online]
fr:
  - { id: FR-0058-01, desc: "budget duro de RAM estimable (N·M·dim·4B) configurable" }
  - { id: FR-0058-02, desc: "sobre budget => ResourceLimit accionable o fallback cuantizado" }
  - { id: FR-0058-03, desc: "cuantizacion escalar/PQ con recall@10 >= 0.95 vs exacto" }
  - { id: FR-0058-04, desc: "bench QPS-recall por bin de selectividad (pre/in/post)" }
nf:
  - { id: NF-0058-01, determinista: mismo seed => mismo indice
    desc: "build determinista con seed fijo (igual que HNSW actual)" }
  - { id: NF-0058-02, desc: "recall@10 exacto sin cuantizar intacto (= 1.0 en el set actual)" }
acceptance_criteria:
  - id: AC-0058-01
    given: un build que excede el budget configurado
    when: se construye el indice
    then: error ResourceLimit accionable con el estimado en bytes
    test: test_ac_0058_01_over_budget_rejected
  - id: AC-0058-02
    given: un indice cuantizado y el exacto de referencia
    when: se mide recall@10 sobre el set de eval
    then: recall >= 0.95
    test: test_ac_0058_02_quantized_recall_at_10
  - id: AC-0058-03
    given: queries por bin de selectividad
    when: se corre el bench QPS-recall
    then: reporta QPS y recall por bin sin crash
    test: test_ac_0058_03_qps_recall_by_selectivity_bin
exit_criteria:
  - cargo test -p ruscadb-vector -- test_ac_0058
  - recall gate >= 0.95 en el set de eval
  - cargo mutants -p ruscadb-vector acotado MS >= 70%
rollback:
  - revertir ruscadb-vector (fallback tras flag/config)
sandbox:
  - cargo test -p ruscadb-vector
---

# SPEC-0058 — Budget RAM HNSW + fallback cuantizado (R3/D4)

## Contexto

Frontera pHNSW (ASP-DAC'26, PCA antes del cómputo exacto) y cuantización
escalar/PQ: el grafo HNSW en RAM escala como `N·M·dim·4B` (R3). Budget duro
configurable con rechazo accionable + fallback cuantizado con gate de recall.
Bench QPS-recall por bin de selectividad para elegir pre/in/post con datos.

## Criterios de aceptación

- **AC-0058-01** — rechazo sobre budget con estimado.
- **AC-0058-02/03** — recall ≥0.95 cuantizado + bench por bin.

## Trazabilidad

Tests `test_ac_0058_<nn>_*`; verificado por `cargo xtask trace`.
