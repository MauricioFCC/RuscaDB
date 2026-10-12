---
id: SPEC-0060
feature: datafusion_adr
status: implemented
owner: arch-team
appetite_days: 4
boundaries:
  crates: [ruscadb-query]
  out_of_scope: [reemplazar el executor, migrar el dialecto, compilar DataFusion por defecto]
fr:
  - { id: FR-0060-01, desc: "ADR-013 que revalida o revierte ADR-001 con evidencia (coste build, arbol, paridad); veredicto registrado en la SSOT del roadmap (docs/adr/ es annexo local-only por ADR_POLICY)" }
  - { id: FR-0060-02, desc: "spike TableProvider sobre filas nativas tras feature datafusion (off por defecto, dependencia opcional)" }
  - { id: FR-0060-03, desc: "differential relacional (WHERE/ORDER BY/proyeccion) vs DataFusion en el subset soportado" }
  - { id: FR-0060-04, desc: "roadmap actualizado con el veredicto" }
nf:
  - { id: NF-0060-01, desc: "cero impacto sin la feature (ni dependencia ni tiempo de build)" }
  - { id: NF-0060-02, desc: "el spike no toca el executor por defecto" }
acceptance_criteria:
  - id: AC-0060-01
    given: la tabla de ADRs del roadmap (docs/RuscaDB-roadmap.md SS4.6)
    when: se lee el ADR de revalidacion (ADR-013)
    then: existe con veredicto unico + tradeoff + evidencia numerica
    test: test_ac_0060_01_adr_recorded
  - id: AC-0060-02
    given: la feature datafusion activada
    when: se escanea una tabla via TableProvider propio
    then: las filas coinciden con el scan nativo (paridad 100%)
    test: test_ac_0060_02_provider_scan_parity
  - id: AC-0060-03
    given: build por defecto (features default)
    when: se inspecciona el arbol de dependencias de ruscadb-query
    then: DataFusion no aparece en el arbol normal
    test: test_ac_0060_03_feature_off_by_default
  - id: AC-0060-04
    given: el diferencial relacional del subset (WHERE + ORDER BY + proyeccion)
    when: la misma consulta corre en DataFusion y en el oraculo nativo
    then: resultados identicos (filas y orden)
    test: test_ac_0060_04_differential_relational
exit_criteria:
  - cargo test -p ruscadb-query --features datafusion -- test_ac_0060
  - roadmap §4.6 con ADR-013 y §Estado con el veredicto
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
differential cubre H7 parcialmente. Cierra con ADR-013 + roadmap actualizado.

### Enmienda 1 (corrección de alcance, 2026-10-10)

- **AC-0060-01**: el ADR de registro vive en la SSOT pusheada
  `docs/RuscaDB-roadmap.md` §4.6 (el directorio `docs/adr/` está *gitignored*
  por la política `ADR_POLICY` y no puede ser leído por CI); el anexo largo es
  local-only opcional.
- **AC-0060-04**: nuevo AC para el differential FR-0060-03 (subset
  relacional WHERE/ORDER BY/proyección), que antes no tenía test.

## Criterios de aceptación

- **AC-0060-01/02/03/04** — ADR con evidencia, paridad del provider,
  off-by-default y differential relacional.

## Trazabilidad

Tests `test_ac_0060_<nn>_*`; verificado por `cargo xtask trace`.
AC-0060-02/04 requieren `--features datafusion` (compilación opcional).
