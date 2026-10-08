---
id: SPEC-0035
feature: blob_gc
status: accepted
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb-multimodal]
  out_of_scope: [integracion en la fachada, GC concurrente, cifrado de la GC]
fr:
  - { id: FR-0035-01, desc: "BlobStore::gc() elimina blobs con refcount == 0" }
  - { id: FR-0035-02, desc: "gc conserva los blobs referenciados (refcount > 0)" }
  - { id: FR-0035-03, desc: "gc devuelve el numero de blobs eliminados y es idempotente" }
  - { id: FR-0035-04, desc: "el barrier de seguridad (sin commits en vuelo) se documenta y es responsabilidad del llamador" }
nf:
  - { id: NF-0035-01, desc: "tras gc, get de un blob vivo sigue funcionando; el eliminado da NotFound" }
  - { id: NF-0035-02, desc: "sin panics; gc en store vacio devuelve 0" }
acceptance_criteria:
  - id: AC-0035-01
    given: un blob con refcount 0 (put + unref)
    when: se ejecuta gc
    then: el blob se elimina y gc devuelve 1
    test: test_ac_0035_01_gc_removes_unreferenced
  - id: AC-0035-02
    given: un blob referenciado
    when: se ejecuta gc
    then: el blob se conserva y gc devuelve 0
    test: test_ac_0035_02_gc_keeps_referenced
  - id: AC-0035-03
    given: un store con blobs vivos y muertos
    when: se ejecuta gc dos veces
    then: la segunda vez devuelve 0 (idempotente) y los vivos siguen accesibles
    test: test_ac_0035_03_gc_is_idempotent
  - id: AC-0035-04
    given: un store vacio
    when: se ejecuta gc
    then: devuelve 0 sin panics
    test: test_ac_0035_04_gc_empty_store
  - id: AC-0035-05
    given: un blob eliminado por gc
    when: se hace get
    then: devuelve NotFound (no basura)
    test: test_ac_0035_05_deleted_blob_is_not_found
exit_criteria:
  - cargo test -p ruscadb-multimodal -- test_ac_0035
  - cargo mutants -p ruscadb-multimodal mutation score >= 70%
rollback:
  - revertir russcadb-multimodal
sandbox:
  - cargo test -p ruscadb-multimodal
---

# SPEC-0035 — GC del blob store (barrier refcount=0)

## Contexto

Riesgo R7 del roadmap: "GC de blobs vs commit en vuelo". El blob store es
content-addressed con refcount (`put`/`unref`). Se añade `BlobStore::gc()` que
elimina los blobs con `refcount == 0`. El **barrier** (no ejecutar GC con commits
en vuelo) se documenta como contrato del llamador; la fachada lo cableará con el
`low_watermark` MVCC en una iteración posterior.

## Criterios de aceptación

- **AC-0035-01/02** — elimina no referenciados, conserva referenciados.
- **AC-0035-03/04** — idempotencia y store vacío.
- **AC-0035-05** — el eliminado da NotFound.

## Trazabilidad

Tests `test_ac_0035_<nn>_*`; verificado por `cargo xtask trace`.
