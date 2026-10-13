---
id: SPEC-0077
feature: schema_migration
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb-txn, ruscadb]
  out_of_scope: [migraciones de datos masivas, downgrade, reescritura de páginas, formato binario del manifiesto]
fr:
  - { id: FR-0077-01, desc: "Manifest::load valida schema_version: > CURRENT => RuscaError::SchemaVersionTooNew { found, supported } (el fichero es válido, hay que actualizar el binario); JSON inválido => CorruptManifest" }
  - { id: FR-0077-02, desc: "runner forward-only: registro de migraciones [(from, to, apply)] aplicado en orden y de forma atómica (tmp + rename) cuando schema_version < CURRENT" }
  - { id: FR-0077-03, desc: "Manifest::migrate_plan(path) es dry-run: devuelve los pasos from->to en orden sin modificar el fichero" }
  - { id: FR-0077-04, desc: "si no hay migración registrada desde una versión antigua => RuscaError::MigrationFailed { from, to, reason } accionable y el fichero queda intacto" }
nf:
  - { id: NF-0077-01, desc: "atómico: nunca un manifiesto a medio migrar (escritura a tmp + rename sobre el original)" }
  - { id: NF-0077-02, desc: "cero dependencias nuevas (std + serde ya presentes); docstrings ES-UTF8; sin panics ni unwrap en producción" }
  - { id: NF-0077-03, desc: "AC-0019-02 de SPEC-0019 se actualiza: la versión futura deja de reportarse como CorruptManifest" }
acceptance_criteria:
  - id: AC-0077-01
    given: un manifiesto con schema_version = CURRENT + 1
    when: se carga con Manifest::load
    then: SchemaVersionTooNew { found, supported } y NO CorruptManifest
    test: test_ac_0077_01_future_version_reports_too_new
  - id: AC-0077-02
    given: un manifiesto con JSON inválido
    when: se carga
    then: sigue siendo CorruptManifest (sin regresión)
    test: test_ac_0077_02_invalid_json_still_corrupt
  - id: AC-0077-03
    given: un manifiesto con schema_version antigua y una migración de prueba registrada (0 -> 1)
    when: se carga con las migraciones registradas
    then: la migración se aplica en orden de forma atómica y el manifiesto persiste con la versión nueva
    test: test_ac_0077_03_migration_applies_atomically
  - id: AC-0077-04
    given: un manifiesto con schema_version antigua sin ninguna migración registrada
    when: se carga
    then: MigrationFailed { from, to, reason } accionable y el fichero queda intacto
    test: test_ac_0077_04_missing_migration_fails_actionably
  - id: AC-0077-05
    given: un manifiesto con schema_version antigua con migraciones registradas
    when: se llama a migrate_plan (dry-run)
    then: devuelve los pasos from->to en orden y NO modifica el fichero
    test: test_ac_0077_05_migrate_plan_is_dry_run
exit_criteria:
  - cargo test -p ruscadb-txn
  - cargo test -p ruscadb --test mvcc_integration  (AC-0019-02 corregido)
  - cargo clippy -p ruscadb-txn --all-targets -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - eliminar migration.rs y la validación de versión de Manifest::load (volver al chequeo inline de la fachada)
sandbox:
  - cargo test -p ruscadb-txn
---

# SPEC-0077 — Migración forward-only de esquema (Ola 0 del plan de producción)

## Contexto

Auditoría *production readiness* 2026-10-12 (riesgo 3): `schema_version` se
valida, pero (a) una versión **futura** se reporta como `CorruptManifest`
(mensaje engañoso: el fichero es válido, el binario es viejo) y (b) no existe
camino de migración ni dry-run.

Frontera 2026 (§3): SQLite usa `user_version` forward-only con regla
«old application opening a newer file should refuse to run»; redb expone
`Database::upgrade()` y separa versiones de formato. Decisión: **partir el
error** (`SchemaVersionTooNew` vs `CorruptManifest` vs `MigrationFailed`),
**runner forward-only atómico** (tmp + rename) y **`migrate_plan()` dry-run**
estilo sqlx/refinery.

## Superficie

- `crates/ruscadb-txn/src/migration.rs` (nuevo): `Migration { from, to, apply }`,
  registro inmutable y `apply_migrations`.
- `Manifest::load` valida versión en `ruscadb-txn` (la fachada propaga vía `?`).
- `Manifest::migrate_plan(path) -> Vec<MigracionPlanificada>` dry-run.
- Variantes ya en `ruscadb-core` (prep Ola 0): `SchemaVersionTooNew`,
  `MigrationFailed`.
- `AC-0019-02` (SPEC-0019) se corrige: versión futura ⇒ `SchemaVersionTooNew`.

## Criterios de aceptación

- **AC-0077-01/02** — error diferenciado «futura» vs «corrupto».
- **AC-0077-03/04** — migración atómica o fallo accionable sin tocar el fichero.
- **AC-0077-05** — dry-run sin escritura.

## Trazabilidad

Tests `test_ac_0077_<nn>_*` con marcador `@spec AC-0077-NN`; verificado por
`cargo xtask trace`/`contract`.
