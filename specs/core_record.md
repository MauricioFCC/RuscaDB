---
id: SPEC-0001
feature: core_record
status: accepted
owner: core-team
appetite_days: 5
boundaries:
  crates: [ruscadb-core]
  out_of_scope: [storage, query, bindings]
fr:
  - { id: FR-0001-01, desc: "Existe un tipo unificado Record con id, scalars, doc, edges, vector, blob y meta" }
  - { id: FR-0001-02, desc: "RecordId es un ULID ordenable lexicograficamente por tiempo de creacion" }
  - { id: FR-0001-03, desc: "Record serializa y deserializa sin perdida de informacion (roundtrip)" }
  - { id: FR-0001-04, desc: "Cada vector embebido lleva la metadata del modelo (model_id, dim, metric)" }
nf:
  - { id: NF-0001-01, desc: "encode+decode de un Record <= 5 us en hardware de referencia" }
  - { id: NF-0001-02, desc: "Record es Send + Sync y no contiene punteros crudos" }
acceptance_criteria:
  - id: AC-0001-01
    given: un Record con los seis campos poblados
    when: se serializa y se deserializa
    then: el resultado es igual al original (roundtrip exacto)
    test: test_ac_0001_01_record_roundtrip_is_lossless
  - id: AC-0001-02
    given: dos RecordId generados en instantes crecientes
    when: se comparan lexicograficamente
    then: el mas antiguo ordena primero (ULID monotono)
    test: test_ac_0001_02_ulid_ordering_is_monotonic
  - id: AC-0001-03
    given: un Embedding con model_id y dim
    when: se construye el Record
    then: la metadata de modelo viaja con el vector (no se puede omitir)
    test: test_ac_0001_03_embedding_metadata_is_mandatory
exit_criteria:
  - cargo test -p ruscadb-core -- test_ac_0001
  - mutation score del diff >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir el crate ruscadb-core a su estado F0 (sin Record)
sandbox:
  - cargo test -p ruscadb-core
---

# SPEC-0001 — Record unificado

## Contexto

`Record` es el tipo fisico unico de RuscaDB: una sola fila que soporta los
cinco modelos (relacional, documento, grafo, vector, time-series) y multimodal
(blob + embedding). Ver `docs/RuscaDB-roadmap.md` §4.4.

## Criterios de aceptacion

- **AC-0001-01** — roundtrip exacto (Invariante I6 del roadmap).
- **AC-0001-02** — ULID ordenable por tiempo (identidad estable y barata).
- **AC-0001-03** — versionado de embeddings obligatorio (ADR-009 / SI-2).

## Trazabilidad

Cada AC se implementa con un test cuyo nombre empieza por `test_ac_0001_<nn>_`
y se anota en el codigo con `// @spec AC-0001-<nn>`.
