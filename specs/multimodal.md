---
id: SPEC-0008
feature: multimodal
status: accepted
owner: index-team
appetite_days: 6
boundaries:
  crates: [ruscadb-multimodal, ruscadb-core, ruscadb-storage]
  out_of_scope: [ai, query, bindings]
fr:
  - { id: FR-0008-01, desc: "blob store content-addressed por SHA-256 con deduplicacion" }
  - { id: FR-0008-02, desc: "refcount por blob; se elimina cuando llega a 0" }
  - { id: FR-0008-03, desc: "get_range devuelve bytes exactos de un rango" }
  - { id: FR-0008-04, desc: "deteccion de media_type por extension" }
nf:
  - { id: NF-0008-01, desc: "put idempotente: mismo contenido => mismo hash" }
  - { id: NF-0008-02, desc: "blobs grandes fuera del buffer pool (streaming)" }
acceptance_criteria:
  - id: AC-0008-01
    given: el mismo contenido insertado dos veces
    when: se hace put de ambos
    then: devuelve el mismo hash y refcount = 2 (un solo blob en disco)
    test: test_ac_0008_01_put_is_content_addressed
  - id: AC-0008-02
    given: un blob almacenado
    when: se lee un rango [a,b)
    then: devuelve exactamente esos bytes
    test: test_ac_0008_02_get_range_returns_bytes
  - id: AC-0008-03
    given: un blob con refcount 1
    when: se hace unref
    then: el blob se elimina del almacen
    test: test_ac_0008_03_unref_removes_blob_at_zero
  - id: AC-0008-04
    given: rutas con distinta extension
    when: se detecta el media_type
    then: devuelve el MIME correcto
    test: test_ac_0008_04_media_type_detection
exit_criteria:
  - cargo test -p ruscadb-multimodal -- test_ac_0008
  - cargo mutants -p ruscadb-multimodal mutation score >= 70%
rollback:
  - revertir ruscadb-multimodal al stub
sandbox:
  - cargo test -p ruscadb-multimodal
---

# SPEC-0008 — Almacén multimodal (blob store CAS)

## Contexto

Blob store content-addressed de RuscaDB (`docs/RuscaDB-roadmap.md` §4.5 y §5.1,
ADR-010). SHA-256 + refcount; deduplicación por hash; sharding `ab/cd/`.

## API esperada

`BlobHash(String)`; `BlobStore::open(dir)`, `put(bytes) -> BlobHash`,
`get_range(hash, Range) -> Vec<u8>`, `ref_count(hash) -> Option<u32>`,
`unref(hash)`, `contains(hash)`, `media_type(path) -> &'static str`.

## Trazabilidad

Tests `test_ac_0008_<nn>_*`; verificado por `cargo xtask trace`.
