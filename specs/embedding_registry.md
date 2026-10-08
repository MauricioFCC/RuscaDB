---
id: SPEC-0032
feature: embedding_registry
status: accepted
owner: ai-team
appetite_days: 5
boundaries:
  crates: [ruscadb-ai, ruscadb, ruscadb-core]
  out_of_scope: [Candle/ONNX real, migracion de vectores entre versiones, persistencia del registro]
fr:
  - { id: FR-0032-01, desc: "ModelRegistry: registra (model_id, dim, metric) allowlisted" }
  - { id: FR-0032-02, desc: "validate(&Embedding) rechaza model_id no registrado o dim/metric incompatibles" }
  - { id: FR-0032-03, desc: "Database::register_model + insert_record valida contra el registro" }
  - { id: FR-0032-04, desc: "embedding_version se fija desde el registro al insertar (ADR-009)" }
nf:
  - { id: NF-0032-01, desc: "sin registro configurado, el comportamiento previo se conserva (compatibilidad)" }
  - { id: NF-0032-02, desc: "sin panics; errores accionables (WHAT+WHERE)" }
acceptance_criteria:
  - id: AC-0032-01
    given: un registro vacio
    when: se valida cualquier embedding
    then: es aceptado (compatibilidad, sin allowlist activa)
    test: test_ac_0032_01_empty_registry_accepts_all
  - id: AC-0032-02
    given: un modelo registrado (model_id, dim, metric)
    when: se valida un embedding que coincide
    then: es aceptado
    test: test_ac_0032_02_registered_model_is_accepted
  - id: AC-0032-03
    given: un modelo registrado
    when: se valida un embedding con model_id distinto
    then: se rechaza con error accionable
    test: test_ac_0032_03_unregistered_model_is_rejected
  - id: AC-0032-04
    given: un modelo registrado
    when: se valida un embedding con dim o metric incompatibles
    then: se rechaza con error accionable
    test: test_ac_0032_04_dimension_or_metric_mismatch_rejected
  - id: AC-0032-05
    given: una Database con un modelo registrado
    when: se hace insert_record con un vector de otro modelo
    then: se rechaza sin escribir; con el modelo correcto se acepta y fija embedding_version
    test: test_ac_0032_05_database_enforces_registry
exit_criteria:
  - cargo test -p ruscadb-ai -p ruscadb -- test_ac_0032
  - cargo mutants -p ruscadb-ai mutation score >= 70%
rollback:
  - revertir russcadb-ai y la fachada al commit previo
sandbox:
  - cargo test -p ruscadb-ai -p ruscadb
---

# SPEC-0032 — Registro de modelos de embedding (allowlist + versionado)

## Contexto

F4 exige: "modelo no allowlisted ⇒ rechazo" y versionado de embeddings
(ADR-009, invariante SI-2). Se añade a `ruscadb-ai` un `ModelRegistry` y la
fachada lo aplica en `insert_record`:

- `ModelRegistry::register(model_id, dim, metric)`, `validate(&Embedding) -> Result<(), RuscaError>`,
  `version_of(model_id) -> Option<u32>`.
- Registro vacío ⇒ acepta todo (compatibilidad con lo existente).
- `Database::register_model(model_id, dim, metric)`; `insert_record` valida el
  vector y fija `meta.embedding_version` desde el registro.

## Criterios de aceptación

- **AC-0032-01/02** — compatibilidad y aceptación.
- **AC-0032-03/04** — rechazo de modelo/dim/metric.
- **AC-0032-05** — aplicación en la fachada.

## Trazabilidad

Tests `test_ac_0032_<nn>_*`; verificado por `cargo xtask trace`.
