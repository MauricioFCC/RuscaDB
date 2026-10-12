---
id: SPEC-0064
feature: ingestion_gates
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-ai]
  out_of_scope: [validacion por tipo en compile-time, antivirus, dedup de registros por contenido]
fr:
  - { id: FR-0064-01, desc: "IngestionPolicy configurable por instancia: max_record_bytes (0 = sin limite) y require_registry (bool)" }
  - { id: FR-0064-02, desc: "persistencia del ModelRegistry en <data>.models.json: register_model sobrevive al reopen y validate sigue pasando" }
nf:
  - { id: NF-0064-01, desc: "cero regresion: con la politica por defecto (0/False) la ingesta actual funciona identica" }
  - { id: NF-0064-02, desc: "errores accionables WHAT+WHY+WHERE (ResourceLimit/Permission), sin panics" }
acceptance_criteria:
  - id: AC-0064-01
    given: max_record_bytes = 1024 y un record cuyo payload serializado supera el limite
    when: se inserta
    then: rechazo ResourceLimit accionable y la tabla queda sin la fila
    test: test_ac_0064_01_oversized_record_rejected
  - id: AC-0064-02
    given: require_registry = true con un modelo registrado (dim 3, L2) y un embedding de otro modelo (dim 5)
    when: se inserta el record con ese embedding
    then: rechazo con detalle (model_id esperado/obtenido, dims) y sin insercion
    test: test_ac_0064_02_unregistered_model_rejected
  - id: AC-0064-03
    given: un modelo registrado via register_model
    when: se cierra y reabre la base
    then: el registry conserva la spec y validate sigue pasando para ese modelo
    test: test_ac_0064_03_registry_persists_across_reopen
  - id: AC-0064-04
    given: una base nueva sin tocar la politica
    when: se insertan records con y sin vector
    then: comporta como hoy (sin limites, sin require registry), cero regresion
    test: test_ac_0064_04_default_policy_is_open
  - id: AC-0064-05
    given: max_record_bytes = 0 y require_registry = false
    when: se inserta un record grande y uno con modelo no registrado
    then: ambos entran (la politica desactivada no filtra nada)
    test: test_ac_0064_05_disabled_policy_allows_all
exit_criteria:
  - cargo test -p ruscadb --test ingestion_gates -- test_ac_0064
  - cargo test -p ruscadb  (suite previa verde)
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - eliminar IngestionPolicy y sus gates; register_model vuelve a memoria (no escribe models.json)
sandbox:
  - cargo test -p ruscadb --test ingestion_gates
---

# SPEC-0064 — Gates de ingesta + registry persistible

## Contexto

Línea futura del roadmap: **gates de ingesta + registry**. Hoy el
`ModelRegistry` (SPEC-0032) valida embeddings si está poblado pero es **solo
memoria** (se pierde al reabrir) y no hay límites de batch/instancia. Esta spec
añade una política de ingesta configurable y la persistencia del registry en un
fichero derivado de la ruta de datos (`<data>.models.json`), reutilizando
`validate_scalars`/`apply_registry` existentes.

## Superficie de API

```rust
// crates/ruscadb (re-exportado)
pub struct IngestionPolicy { pub max_record_bytes: usize, pub require_registry: bool }
impl Default for IngestionPolicy { /* 0, false */ }
impl Database {
    pub fn set_ingestion_policy(&mut self, policy: IngestionPolicy);
    pub fn ingestion_policy(&self) -> IngestionPolicy;
}
```

Gates: (1) en `insert`/`insert_record`, si `max_record_bytes > 0`, serializar el
`Record` (postcard) y rechazar si supera el límite (`RuscaError::ResourceLimit`);
(2) en `insert_record`, si `require_registry` y el registry **no está vacío**,
aplicar `validate` (rechazo con detalle). `register_model` persiste el registry
en `<data>.models.json` (JSON serde); `open` lo carga si existe.

## Criterios de aceptación

- **AC-0064-01/02** — gates activos rechazan con detalle accionable.
- **AC-0064-03** — registry persistente entre reopens.
- **AC-0064-04/05** — política por defecto abierta (cero regresión).

## Trazabilidad

Tests `test_ac_0064_<nn>_*`; verificado por `cargo xtask trace`/`contract`.