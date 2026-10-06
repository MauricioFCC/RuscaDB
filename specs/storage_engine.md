---
id: SPEC-0003
feature: storage_engine
status: accepted
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb-storage, ruscadb-core]
  out_of_scope: [wal, query, bindings, ai]
fr:
  - { id: FR-0003-01, desc: "El buffer pool nunca excede su presupuesto de marcos (SI-4)" }
  - { id: FR-0003-02, desc: "Las páginas pinneadas nunca se desalojan" }
  - { id: FR-0003-03, desc: "El desalojo sigue la política LRU-K (K=2)" }
  - { id: FR-0003-04, desc: "PagedFile persiste páginas de tamaño fijo y las recupera sin pérdida" }
nf:
  - { id: NF-0003-01, desc: "get/put de página en memoria O(1) amortizado" }
  - { id: NF-0003-02, desc: "presupuesto RAM = capacity * 4 KiB, duro" }
acceptance_criteria:
  - id: AC-0003-01
    given: un buffer pool de capacidad 2
    when: se solicitan 5 páginas distintas y se despinnean
    then: el número de marcos en memoria nunca supera 2
    test: test_ac_0003_01_buffer_pool_respects_budget
  - id: AC-0003-02
    given: un buffer pool de capacidad 1 con su única página pinneada
    when: se solicita otra página
    then: devuelve BufferPoolFull (nunca desaloja una página pinneada)
    test: test_ac_0003_02_pinned_page_is_not_evicted
  - id: AC-0003-03
    given: una página desalojada
    when: se vuelve a solicitar
    then: se recarga desde el loader y el dato se conserva
    test: test_ac_0003_03_evicted_page_reloads
  - id: AC-0003-04
    given: páginas A (2 referencias) y B (1 referencia) en un pool de capacidad 2
    when: se inserta C y hay que desalojar
    then: se desaloja B (menos referencias), no A (LRU-2)
    test: test_ac_0003_04_lru_k_discriminates
  - id: AC-0003-05
    given: páginas escritas en un PagedFile
    when: se hace flush, se reabre el archivo y se leen
    then: el contenido coincide byte a byte (roundtrip)
    test: test_ac_0003_05_paged_file_roundtrip
exit_criteria:
  - cargo test -p ruscadb-storage -- test_ac_0003
  - cargo mutants -p ruscadb-storage mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir ruscadb-storage al estado F1 (crate vacío)
sandbox:
  - cargo test -p ruscadb-storage
---

# SPEC-0003 — Storage Engine (Buffer Pool + PagedFile)

## Contexto

Primer componente real de almacenamiento de RuscaDB (`docs/RuscaDB-roadmap.md`
§5.5/§5.6, ADR-003/ADR-008). El buffer pool mantiene páginas de 4 KiB en
memoria con **presupuesto RAM duro** y desalojo **LRU-K (K=2)**; `PagedFile`
es el almacén de páginas de tamaño fijo en disco. La integración con el WAL y
el manifiesto llega en la siguiente iteración.

## Criterios de aceptación

- **AC-0003-01** — presupuesto duro (invariante SI-4).
- **AC-0003-02** — páginas pinneadas no desalojables; backpressure con error.
- **AC-0003-03** — recarga transparente desde el loader.
- **AC-0003-04** — LRU-K discrimina frecuencia (O'Neil 1993).
- **AC-0003-05** — persistencia fiel de `PagedFile`.

## Trazabilidad

Cada AC se implementa con un test `test_ac_0003_<nn>_*` y se anota con
`// @spec AC-0003-<nn>`. Property tests (proptest) cubren los invariantes.
