---
id: SPEC-0021
feature: filtered_vector_search
status: accepted
owner: search-team
appetite_days: 6
boundaries:
  crates: [ruscadb-fvs, ruscadb-vector, ruscadb-core]
  out_of_scope: [integracion en el executor, PCA/pHNSW, PQ]
fr:
  - { id: FR-0021-01, desc: "selectividad s = |filtro| / N y eleccion de estrategia por umbrales (0.6 / 0.05)" }
  - { id: FR-0021-02, desc: "pre-filtering: filtra candidatos y hace top-k exacto sobre ellos" }
  - { id: FR-0021-03, desc: "post-filtering: top-k del indice y luego filtra por el predicado" }
  - { id: FR-0021-04, desc: "iFVS (in-filter): top-k exacto aplicando el filtro durante la busqueda" }
  - { id: FR-0021-05, desc: "nunca devuelve ids fuera del filtro (sonido) en ninguna estrategia" }
nf:
  - { id: NF-0021-01, desc: "pre-filter e iFVS coinciden con la fuerza bruta filtrada (exactitud)" }
  - { id: NF-0021-02, desc: "sin panics; k=0 o filtro vacio => vacio" }
acceptance_criteria:
  - id: AC-0021-01
    given: distintas selectividades
    when: se elige la estrategia
    then: s>=0.6 => PostFilter; 0.05<=s<0.6 => InFilter; s<0.05 => PreFilter
    test: test_ac_0021_01_strategy_by_selectivity
  - id: AC-0021-02
    given: un corpus y un filtro
    when: se hace pre-filtering
    then: devuelve el top-k exacto restringido al filtro
    test: test_ac_0021_02_pre_filter_is_exact
  - id: AC-0021-03
    given: un corpus y un filtro
    when: se hace post-filtering
    then: todos los ids devueltos pertenecen al filtro (nunca ids excluidos)
    test: test_ac_0021_03_post_filter_is_sound
  - id: AC-0021-04
    given: un corpus y un filtro
    when: se hace iFVS
    then: coincide con el pre-filtering (exacto)
    test: test_ac_0021_04_ifvs_matches_pre_filter
  - id: AC-0021-05
    given: filtro vacio, filtro total, k=0 y k>N
    when: se busca
    then: devuelve vacio / todo / acotado segun corresponda, sin panics
    test: test_ac_0021_05_boundary_filters
exit_criteria:
  - cargo test -p ruscadb-fvs -- test_ac_0021
  - cargo mutants -p ruscadb-fvs mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - eliminar el crate ruscadb-fvs del workspace
sandbox:
  - cargo test -p ruscadb-fvs
---

# SPEC-0021 — Filtrado vectorial híbrido (FVS: pre/post/iFVS)

## Contexto

El roadmap (§5.4) y la evidencia iFVS (arXiv:2607.22922) establecen que ninguna
estrategia domina; se elige por selectividad `s = |filtro| / N`:

- `s >= 0.6` → **post-filtering** (busca top-k y filtra).
- `0.05 <= s < 0.6` → **iFVS / in-filter** (aplica el filtro durante la búsqueda).
- `s < 0.05` → **pre-filtering** (filtra candidatos y busca en ellos).

Se implementa en un crate aislado `ruscadb-fvs` sobre `ruscadb-vector`
(`distance`) y `ruscadb-core` (`Metric`). La integración en el executor
(`KNN ... WHERE`) es la iteración siguiente.

## Criterios de aceptación

- **AC-0021-01** — selección por umbrales.
- **AC-0021-02/04** — pre-filter e iFVS exactos.
- **AC-0021-03** — sonido (nunca ids fuera del filtro).
- **AC-0021-05** — fronteras sin panics.

## Trazabilidad

Tests `test_ac_0021_<nn>_*` + proptests; verificado por `cargo xtask trace`.
