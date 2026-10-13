//! Migración **forward-only** del esquema del manifiesto (SPEC-0077).
//!
//! Un [`Migration`] describe un único salto `from -> to` y su transformación
//! `apply`. El runner [`apply_migrations`] encadena saltos hasta la versión
//! objetivo y [`plan_migrations`] calcula el plan (dry-run) sin aplicar nada.
//!
//! La política es la de SQLite `user_version` / redb: **solo se avanza**. Un
//! fichero con `schema_version` más nueva que la soportada no es corrupción
//! (lo resuelve [`crate::Manifest::load`] con
//! [`RuscaError::SchemaVersionTooNew`]); un salto sin migración registrada es
//! accionable ([`RuscaError::MigrationFailed`]) y deja el fichero intacto.

use ruscadb_core::RuscaError;

use crate::manifest::Manifest;

/// Transformación pura aplicada a un manifiesto durante una migración.
///
/// Recibe el manifiesto en la versión `from` (ya cargado en memoria) y lo deja
/// listo para la versión `to`. La versión de esquema la fija el runner, no
/// `apply`: así el avance de versión es consistente aunque `apply` olvide
/// tocarla.
pub type MigrationApply = fn(&mut Manifest) -> Result<(), RuscaError>;

/// Una migración inmutable `from -> to` del esquema del manifiesto.
#[derive(Clone, Copy)]
pub struct Migration {
    /// Versión de esquema de origen.
    pub from: u32,
    /// Versión de esquema de destino (`to > from`).
    pub to: u32,
    /// Transformación que lleva el manifiesto de `from` a `to`.
    pub apply: MigrationApply,
}

/// Paso planificado de una migración (salida del dry-run).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedMigration {
    /// Versión de esquema de origen del paso.
    pub from: u32,
    /// Versión de esquema de destino del paso.
    pub to: u32,
}

/// Calcula el plan ordenado de migraciones de `from` a `target` (dry-run).
///
/// Ordena la cadena encadenando el salto cuyo `from` coincide con la versión
/// actual, sin aplicar ninguna transformación ni tocar disco. Los saltos deben
/// ser estrictamente crecientes (`to > current`) para garantizar terminación.
///
/// Args:
///     from: Versión de esquema de partida.
///     migrations: Registro de migraciones disponibles.
///     target: Versión de esquema objetivo.
///
/// Returns:
///     Los pasos `from -> to` en orden; vacío si `from == target`.
///
/// Raises:
///     [`RuscaError::MigrationFailed`] si `from > target` (no hay downgrade), si
///     falta el salto desde alguna versión intermedia, o si un salto se pasa del
///     destino (forward-only).
pub fn plan_migrations(
    from: u32,
    migrations: &[Migration],
    target: u32,
) -> Result<Vec<PlannedMigration>, RuscaError> {
    if from > target {
        return Err(RuscaError::MigrationFailed {
            from,
            to: target,
            reason: format!(
                "no se soporta downgrade: schema_version {from} es más nueva que el destino {target}"
            ),
        });
    }

    let mut steps = Vec::new();
    let mut current = from;
    while current < target {
        let Some(migration) = migrations
            .iter()
            .find(|candidate| candidate.from == current && candidate.to > current)
        else {
            return Err(RuscaError::MigrationFailed {
                from: current,
                to: target,
                reason: format!(
                    "no hay migración registrada desde schema_version {current} hacia {target}; \
                     registra el salto {current} -> ? o actualiza el binario"
                ),
            });
        };
        if migration.to > target {
            return Err(RuscaError::MigrationFailed {
                from: current,
                to: target,
                reason: format!(
                    "la migración {from} -> {to} se pasa del destino {target} (forward-only)",
                    from = migration.from,
                    to = migration.to
                ),
            });
        }
        steps.push(PlannedMigration {
            from: migration.from,
            to: migration.to,
        });
        current = migration.to;
    }
    Ok(steps)
}

/// Aplica en orden las migraciones de `manifest` hasta `target` mutando en memoria.
///
/// Planifica primero ([`plan_migrations`]) y luego ejecuta cada `apply`
/// actualizando `schema_version` al destino del paso. Si falta un salto o su
/// `apply` falla, devuelve [`RuscaError::MigrationFailed`]; el llamante solo
/// persiste el manifiesto si esta función devuelve `Ok`, de modo que un fallo
/// nunca deja un fichero a medio migrar.
///
/// Args:
///     manifest: Manifiesto cargado en memoria (se muta si todo va bien).
///     migrations: Registro de migraciones disponibles.
///     target: Versión de esquema objetivo.
///
/// Returns:
///     `Ok(())` cuando `manifest` queda en `target`.
///
/// Raises:
///     [`RuscaError::MigrationFailed`] si el plan está incompleto o un `apply`
///     falla (WHAT + WHY + WHERE en `reason`).
pub fn apply_migrations(
    manifest: &mut Manifest,
    migrations: &[Migration],
    target: u32,
) -> Result<(), RuscaError> {
    let steps = plan_migrations(manifest.schema_version, migrations, target)?;
    for step in steps {
        let migration = migrations
            .iter()
            .find(|candidate| candidate.from == step.from && candidate.to == step.to)
            .ok_or_else(|| RuscaError::MigrationFailed {
                from: step.from,
                to: target,
                reason: format!(
                    "el plan incluyó {from} -> {to} pero la migración no está registrada",
                    from = step.from,
                    to = step.to
                ),
            })?;
        (migration.apply)(manifest).map_err(|error| RuscaError::MigrationFailed {
            from: step.from,
            to: target,
            reason: format!(
                "la migración {from} -> {to} falló: {error}",
                from = step.from,
                to = step.to
            ),
        })?;
        manifest.schema_version = step.to;
    }
    Ok(())
}
