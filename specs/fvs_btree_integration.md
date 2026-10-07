---
id: SPEC-0024
feature: fvs_btree_integration
status: accepted
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-fvs, ruscadb-btree]
  out_of_scope: [indice primario persistido en paginas, iFVS sobre HNSW real, GC]
fr:
  - { id: FR-0024-01, desc: "el KNN con filtro WHERE usa ruscadb-fvs (estrategia por selectividad)" }
  - { id: FR-0024-02, desc: "indice primario en memoria RecordId -> locator con ruscadb-btree" }
  - { id: FR-0024-03, desc: "Database::delete usa el indice primario (O(log n)) en vez de scan del heap" }
  - { id: FR-0024-04, desc: "el indice primario se reconstruye al abrir (durabilidad)" }
  - { id: FR-0024-05, desc: "coherencia indice<->datos en insert/delete/reopen" }
nf:
  - { id: NF-0024-01, desc: "KNN+WHERE devuelve el top-k exacto restringido al filtro (pre/in)" }
  - { id: NF-0024-02, desc: "sin panics; id ausente o indice vacio se manejan como no-op" }
acceptance_criteria:
  - id: AC-0024-01
    given: una tabla con vectores y un filtro WHERE
    when: se ejecuta "SELECT * FROM t KNN embedding <|k|> [..] WHERE a > x"
    then: devuelve el top-k exacto restringido al filtro (coincide con fuerza bruta filtrada)
    test: test_ac_0024_01_knn_with_filter_uses_fvs
  - id: AC-0024-02
    given: filas insertadas
    when: se borra una por id
    then: el borrado es correcto y usa el indice primario (no scan completo)
    test: test_ac_0024_02_primary_index_point_delete
  - id: AC-0024-03
    given: un indice primario construido
    when: se cierra y reabre
    then: el indice se reconstruye y el point lookup sigue funcionando
    test: test_ac_0024_03_primary_index_survives_reopen
  - id: AC-0024-04
    given: una fila borrada
    when: se hace point lookup por id
    then: no aparece; y reinsertar el mismo id vuelve a registrarlo
    test: test_ac_0024_04_delete_then_reinsert_same_id
  - id: AC-0024-05
    given: un id ausente o un indice vacio
    when: se opera
    then: no-op sin panics y el indice queda consistente
    test: test_ac_0024_05_index_edge_cases
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0024
  - cargo mutants -p ruscadb mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir la fachada al commit previo (indice secundario sin primario)
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0024 — Integración de FVS y B+tree en la fachada

## Contexto

Cierra dos cabos sueltos de F3 en el composition root:

- **FVS en el executor**: cuando un `KNN` lleva `WHERE`, el filtro se resuelve
  con `ruscadb-fvs` (estrategia por selectividad) en vez de post-filtrar siempre;
  el resultado debe ser el top-k exacto restringido al filtro.
- **Índice primario B+tree**: `ruscadb-btree` mantiene `RecordId -> locator` en
  memoria; `Database::delete` (SPEC-0022) lo usa para localizar la fila en
  O(log n) en vez de escanear el heap. Se reconstruye al abrir.

## Criterios de aceptación

- **AC-0024-01** — KNN+WHERE exacto con FVS.
- **AC-0024-02/03** — índice primario y su persistencia.
- **AC-0024-04/05** — coherencia y fronteras.

## Trazabilidad

Tests `test_ac_0024_<nn>_*` en `ruscadb`; verificado por `cargo xtask trace`.
