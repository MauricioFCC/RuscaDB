---
id: SPEC-0060
feature: datafusion_adr
status: accepted
owner: arch-team
appetite_days: 4
boundaries:
  crates: [ruscadb-query]
  out_of_scope: [reemplazar el executor, migrar el dialecto, compilar DataFusion por defecto]
fr:
  - { id: FR-0060-01, desc: "ADR que revalida o revierte ADR-001 con evidencia (coste build, binario, paridad)" }
  - { id: FR-0060-02, desc: "spike TableProvider sobre el heap tras feature datafusion (off por defecto)" }
  - { id: FR-0060-03, desc: "differential relacional vs DataFusion en el subset soportado" }
  - { id: FR-0060-04, desc: "roadmap actualizado con el veredicto" }
nf:
  - { id: NF-0060-01, desc: "cero impacto sin la feature (ni dependencia ni tiempo de build)" }
  - { id: NF-0060-02, desc: "el spike no toca el executor por defecto" }
acceptance_criteria:
  - id: AC-0060-01
    given: docs/adr/
    when: se lee el ADR de revalidacion
    then: existe con veredicto unico + tradeoff + evidencia numerica
    test: test_ac_0060_01_adr_recorded
  - id: AC-0060-02
    given: la feature datafusion activada
    when: se escanea una tabla via TableProvider
    then: las filas coinciden con el scan nativo
    test: test_ac_0060_02_provider_scan_parity
  - id: AC-0060-03
    given: build por defecto
    when: se compila el workspace
    then: DataFusion no aparece en el arbol de dependencias
    test: test_ac_0060_03_feature_off_by_default
exit_criteria:
  - cargo test -p ruscadb-query -- test_ac_0060
  - roadmap §Estado actualizado
rollback:
  - revertir query + docs (feature off, spike aislado)
sandbox:
  - cargo test -p ruscadb-query -- test_ac_0060
---

# SPEC-0060 — Revalidación ADR-001/DataFusion

## Contexto

La desviación DataFusion está documentada como ⏳ pero sin veredicto. Patrón
frontera: DataFusion como librería embebida (`TableProvider` + `SessionContext`)
detrás del `QueryPort` propio, sin reescribir el dialecto RQL. El spike vive
tras feature `datafusion` (off por defecto, dependencia opcional) y el
differential cubre H7 parcialmente. Cierra con ADR + roadmap actualizado.

## Criterios de aceptación

- **AC-0060-01/02/03** — ADR con evidencia, paridad del provider, off-by-default.

## Trazabilidad

Tests `test_ac_0060_<nn>_*`; verificado por `cargo xtask trace`.
