---
id: SPEC-0030
feature: api_hardening
status: accepted
owner: architecture-team
appetite_days: 4
boundaries:
  crates: [ruscadb]
  out_of_scope: [cambios de comportamiento, nuevas APIs, otros crates]
fr:
  - { id: FR-0030-01, desc: "los helpers heap_*/index_*/execute_select*/plan_for pasan a pub(crate)" }
  - { id: FR-0030-02, desc: "RowLocator y CATALOG_MAGIC/VERSION pasan a pub(crate)" }
  - { id: FR-0030-03, desc: "lib.rs solo re-exporta la superficie MVP (Database, tipos, Page/PageId)" }
  - { id: FR-0030-04, desc: "los tests que usaban index_lookup_eq usan execute (API publica)" }
  - { id: FR-0030-05, desc: "get_record/primary_index_len/read_page/write_page siguen publicos (introspeccion MVP)" }
nf:
  - { id: NF-0030-01, desc: "cero regresiones: todos los tests previos siguen verdes" }
  - { id: NF-0030-02, desc: "la API publica documentada en docs/MVP.md coincide con lib.rs" }
acceptance_criteria:
  - id: AC-0030-01
    given: la superficie publica de la fachada
    when: se inspecciona lib.rs
    then: no re-exporta heap_*/index_*/execute_select*/plan_for/RowLocator/CATALOG_*
    test: test_ac_0030_01_public_surface_has_no_internals
  - id: AC-0030-02
    given: los tests de mvcc_soft_delete
    when: se ejecutan
    then: usan execute (no index_lookup_eq) y pasan
    test: test_ac_0030_02_index_tests_use_public_api
  - id: AC-0030-03
    given: la fachada endurecida
    when: se compila y testea
    then: T1 verde (sin regresiones) y ffi sigue compilando (usa Page/PageId)
    test: test_ac_0030_03_no_regressions
exit_criteria:
  - cargo test -p ruscadb -p ruscadb-ffi
  - cargo mutants -p ruscadb mutation score >= 70%
rollback:
  - revertir la fachada al commit previo
sandbox:
  - cargo test -p ruscadb -p ruscadb-ffi
---

# SPEC-0030 — Endurecimiento de la API pública (MVP)

## Contexto

La fachada filtra **internals** (`heap_*`, `index_*`, `execute_select*`,
`plan_for`, `RowLocator`, `CATALOG_*`) por sus re-exports. Como arquitecto
senior, la superficie pública del MVP debe ser solo `Database` + tipos de
dominio; los helpers pasan a `pub(crate)`. Se mantienen públicos por necesidad
MVP: `Page`/`PageId`/`PAGE_SIZE` (los usa `ruscadb-ffi`), `get_record` y
`primary_index_len` (introspección usada por tests), y todos los métodos de
`Database`.

- Los 5 usos de `index_lookup_eq` en `tests/mvcc_soft_delete.rs` se migran a
  `execute("SELECT * FROM t WHERE a = N")` (equivalente observable).
- Verificación de superficie: un test que aserta la **ausencia** de los
  símbolos internos en la API pública (vía intento de resolución o lista
  documentada).

## Criterios de aceptación

- **AC-0030-01** — superficie sin internals.
- **AC-0030-02** — tests migrados a la API pública.
- **AC-0030-03** — cero regresiones (incl. ffi).

## Trazabilidad

Tests `test_ac_0030_<nn>_*`; verificado por `cargo xtask trace`.
