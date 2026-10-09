---
id: SPEC-0050
feature: docs_release
status: accepted
owner: product-team
appetite_days: 4
boundaries:
  crates: []
  out_of_scope: [codigo, workflows]
fr:
  - { id: FR-0050-01, desc: "CHANGELOG.md (Keep a Changelog) con las fases/specs implementadas" }
  - { id: FR-0050-02, desc: "docs/RuscaDB-roadmap.md: seccion 'Estado de implementacion' con lo hecho vs pendiente" }
  - { id: FR-0050-03, desc: "docs/MVP.md actualizado (capacidades: DML, docs, group by, order by, blob, TS)" }
  - { id: FR-0050-04, desc: "README: seccion de estado/cobertura actualizada" }
nf:
  - { id: NF-0050-01, desc: "documentacion coherente con la API publica y las specs existentes" }
  - { id: NF-0050-02, desc: "sin enlaces muertos a specs inexistentes" }
acceptance_criteria:
  - id: AC-0050-01
    given: CHANGELOG.md
    when: se lee
    then: existe con formato Keep a Changelog y lista las specs/fases
    test: test_ac_0050_01_changelog_exists
  - id: AC-0050-02
    given: docs/RuscaDB-roadmap.md
    when: se lee
    then: incluye una seccion 'Estado de implementacion'
    test: test_ac_0050_02_roadmap_status
  - id: AC-0050-03
    given: docs/MVP.md
    when: se lee
    then: menciona DML, GROUP BY, ORDER BY y blobs
    test: test_ac_0050_03_mvp_updated
exit_criteria:
  - cargo test -p xtask -- test_ac_0050
rollback:
  - revertir los documentos
sandbox:
  - cargo test -p xtask -- test_ac_0050
---

# SPEC-0050 — Documentación de estado y release

## Contexto

Cierra la trazabilidad de producto: `CHANGELOG.md`, sección de estado en el
roadmap, `docs/MVP.md` actualizado y README con la cobertura actual. Verificado
por tests en `crates/xtask` (lectura de los documentos).

## Criterios de aceptación

- **AC-0050-01..03** — CHANGELOG, estado del roadmap y MVP.

## Trazabilidad

Tests `test_ac_0050_<nn>_*`; verificado por `cargo xtask trace`.
