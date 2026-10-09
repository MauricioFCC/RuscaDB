---
id: SPEC-0048
feature: ifvs_executor
status: accepted
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-fvs]
  out_of_scope: [nuevos indices, pHNSW, PQ]
fr:
  - { id: FR-0048-01, desc: "el KNN con WHERE usa ruscadb_fvs::search_auto_indexed (estrategia por selectividad) sobre el HNSW" }
  - { id: FR-0048-02, desc: "pre/in devuelven el top-k exacto filtrado; post es sonoro (solo ids permitidos)" }
  - { id: FR-0048-03, desc: "sin filtro, el KNN sigue usando el indice HNSW sin cambios" }
  - { id: FR-0048-04, desc: "documenta la estrategia elegida (pre/in/post) en el codigo" }
nf:
  - { id: NF-0048-01, desc: "sin regresion: KNN sin filtro y KNN con filtro devuelven resultados correctos" }
  - { id: NF-0048-02, desc: "sin panics; filtro vacio => vacio" }
acceptance_criteria:
  - id: AC-0048-01
    given: una tabla con vectores y un filtro moderado
    when: se ejecuta KNN+WHERE
    then: el resultado coincide con la fuerza bruta filtrada (pre/in exactos)
    test: test_ac_0048_01_knn_filter_uses_ifvs_exact
  - id: AC-0048-02
    given: un filtro muy selectivo
    when: se ejecuta KNN+WHERE
    then: devuelve exactamente el top-k restringido al filtro
    test: test_ac_0048_02_selective_filter_exact
  - id: AC-0048-03
    given: un filtro total (todos los ids)
    when: se ejecuta KNN+WHERE
    then: coincide con el KNN sin filtro
    test: test_ac_0048_03_full_filter_matches_unfiltered
  - id: AC-0048-04
    given: un filtro vacio
    when: se ejecuta KNN+WHERE
    then: devuelve vacio sin panics
    test: test_ac_0048_04_empty_filter
  - id: AC-0048-05
    given: el KNN sin WHERE
    when: se ejecuta
    then: sin regresion respecto al comportamiento previo
    test: test_ac_0048_05_knn_without_filter_unchanged
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0048
  - cargo mutants -p ruscadb mutation score >= 70%
rollback:
  - revertir la fachada
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0048 — iFVS en el executor (KNN + WHERE)

## Contexto

El executor pre-filtra exacto. Se cablea `ruscadb_fvs::search_auto_indexed`
(iFVS sobre HNSW, SPEC-0047) cuando el `KNN` lleva `WHERE`: pre/in exactos, post
sonoro, elegido por selectividad. Sin filtro, comportamiento previo intacto.

## Criterios de aceptación

- **AC-0048-01..03** — exactitud/sonido por selectividad.
- **AC-0048-04/05** — fronteras y no regresión.

## Trazabilidad

Tests `test_ac_0048_<nn>_*`; verificado por `cargo xtask trace`.
