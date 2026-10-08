---
id: SPEC-0038
feature: blob_integration
status: accepted
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-multimodal]
  out_of_scope: [cifrado del blob store en la fachada, streaming, GC concurrente]
fr:
  - { id: FR-0038-01, desc: "DbConfig.blob_path: Option<PathBuf> abre/crea el blob store junto a la base" }
  - { id: FR-0038-02, desc: "Database::put_blob/get_blob delegan en el BlobStore y devuelven el BlobHash" }
  - { id: FR-0038-03, desc: "Database::gc_blobs() ejecuta el GC con el barrier (error si hay tx activa)" }
  - { id: FR-0038-04, desc: "sin blob_path, put/get/gc_blobs devuelven error accionable (no configurado)" }
nf:
  - { id: NF-0038-01, desc: "el barrier R7: gc_blobs solo con active_tx == None (sin commits en vuelo)" }
  - { id: NF-0038-02, desc: "put_blob/get_blob roundtrip exacto; dedup por contenido preservada" }
acceptance_criteria:
  - id: AC-0038-01
    given: una DbConfig con blob_path
    when: se hace put_blob y get_blob
    then: se recuperan los bytes exactos y el hash es estable
    test: test_ac_0038_01_blob_roundtrip
  - id: AC-0038-02
    given: una base sin blob_path
    when: se llama put_blob
    then: error accionable (blob store no configurado)
    test: test_ac_0038_02_blob_not_configured
  - id: AC-0038-03
    given: blobs sin referencias
    when: se llama gc_blobs
    then: se eliminan y devuelve el conteo
    test: test_ac_0038_03_gc_blobs_removes_orphans
  - id: AC-0038-04
    given: una transaccion activa
    when: se llama gc_blobs
    then: devuelve error (barrier R7: sin commits en vuelo)
    test: test_ac_0038_04_gc_blobs_blocked_with_active_tx
  - id: AC-0038-05
    given: dos put_blob del mismo contenido
    when: se guardan
    then: deduplican (mismo hash) y gc_blobs no elimina un blob referenciado
    test: test_ac_0038_05_dedup_and_gc_safety
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0038
  - cargo mutants -p ruscadb mutation score >= 70%
rollback:
  - revertir la fachada
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0038 — Integración del blob store en la fachada (F4)

## Contexto

Hoy los blobs viven en un `BlobStore` externo. Se integra en la fachada:
`DbConfig.blob_path` abre/crea el store; `Database::put_blob/get_blob` delegan;
`Database::gc_blobs()` ejecuta el GC **con el barrier R7** (solo si
`active_tx().is_none()`, si no error accionable). Sin `blob_path`, las
operaciones devuelven error accionable.

## Criterios de aceptación

- **AC-0038-01/02** — roundtrip y no configurado.
- **AC-0038-03/04** — GC y barrier.
- **AC-0038-05** — dedup + seguridad del GC.

## Trazabilidad

Tests `test_ac_0038_<nn>_*`; verificado por `cargo xtask trace`.
