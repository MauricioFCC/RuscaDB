//! Manifiesto versionado de RuscaDB (ADR-010).
//!
//! El manifiesto es el *entrypoint* atómico del directorio de base: describe la
//! versión de esquema, la época lógica (bump en cada `PUBLISH`) y el LSN de
//! checkpoint. Se escribe con `tmp + fsync + rename` para que un lector nunca
//! observe un estado a medias.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use ruscadb_core::RuscaError;
use serde::{Deserialize, Serialize};

use crate::migration::{Migration, PlannedMigration, apply_migrations, plan_migrations};

/// Versión de esquema actual del manifiesto.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Sufijo del fichero temporal usado en la escritura atómica.
const TMP_SUFFIX: &str = ".tmp";

/// Estado versionado de la base (entrypoint atómico).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Versión del esquema lógico (migraciones inmutables).
    pub schema_version: u32,
    /// Época lógica; se incrementa en cada publicación atómica.
    pub epoch: u64,
    /// LSN de checkpoint desde el que se reproduce el WAL.
    pub checkpoint_lsn: u64,
}

impl Manifest {
    /// Crea el manifiesto inicial.
    ///
    /// Returns:
    ///     Manifiesto con [`CURRENT_SCHEMA_VERSION`], `epoch = 0` y
    ///     `checkpoint_lsn = 0`.
    #[allow(clippy::new_without_default)] // el default neutro sería schema_version=0 (inválido)
    pub fn new() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            epoch: 0,
            checkpoint_lsn: 0,
        }
    }

    /// Carga y valida el manifiesto de `path`.
    ///
    /// La validación distingue tres casos (SPEC-0077): JSON inválido
    /// ([`RuscaError::CorruptManifest`]), versión futura
    /// ([`RuscaError::SchemaVersionTooNew`]) y versión antigua sin migración
    /// disponible ([`RuscaError::MigrationFailed`]). Equivale a
    /// [`Manifest::load_with_migrations`] con un registro vacío.
    ///
    /// Args:
    ///     path: Ruta del fichero `MANIFEST.json`.
    ///
    /// Returns:
    ///     El manifiesto deserializado en [`CURRENT_SCHEMA_VERSION`].
    ///
    /// Raises:
    ///     [`RuscaError::CorruptManifest`] si el JSON es inválido o faltan
    ///     campos obligatorios.
    ///     [`RuscaError::SchemaVersionTooNew`] si declara una versión más nueva
    ///     que la soportada por este binario.
    ///     [`RuscaError::MigrationFailed`] si declara una versión antigua y no
    ///     hay migración registrada.
    ///     [`RuscaError::Io`] si el fichero no se puede leer.
    pub fn load(path: &Path) -> Result<Self, RuscaError> {
        Self::load_with_migrations(path, &[])
    }

    /// Carga el manifiesto aplicando migraciones forward-only si hacen falta.
    ///
    /// Lee y valida el manifiesto; si su versión es anterior a
    /// [`CURRENT_SCHEMA_VERSION`], aplica la cadena de `migrations` en orden
    /// ([`apply_migrations`]) y **persiste** el resultado de forma atómica
    /// (`tmp` + `rename`). Si algo falla, el fichero original queda intacto.
    ///
    /// Args:
    ///     path: Ruta del fichero `MANIFEST.json`.
    ///     migrations: Registro de migraciones `[(from, to, apply)]`.
    ///
    /// Returns:
    ///     El manifiesto en [`CURRENT_SCHEMA_VERSION`] (migrado y persistido si
    ///     el fichero tenía una versión antigua).
    ///
    /// Raises:
    ///     [`RuscaError::CorruptManifest`] si el JSON es inválido.
    ///     [`RuscaError::SchemaVersionTooNew`] si la versión es futura.
    ///     [`RuscaError::MigrationFailed`] si falta una migración o su `apply`
    ///     falla (el fichero original queda intacto).
    ///     [`RuscaError::Io`] si la lectura, la escritura o el `rename` fallan.
    pub fn load_with_migrations(path: &Path, migrations: &[Migration]) -> Result<Self, RuscaError> {
        let mut manifest = Self::read_validated(path)?;
        if manifest.schema_version < CURRENT_SCHEMA_VERSION {
            apply_migrations(&mut manifest, migrations, CURRENT_SCHEMA_VERSION)?;
            manifest.store(path)?;
        }
        Ok(manifest)
    }

    /// Planifica (dry-run) las migraciones necesarias sin tocar el fichero.
    ///
    /// Lee y valida el manifiesto y calcula, sin escribir, los pasos `from -> to`
    /// que llevarían la versión en disco hasta [`CURRENT_SCHEMA_VERSION`].
    ///
    /// Args:
    ///     path: Ruta del fichero `MANIFEST.json`.
    ///     migrations: Registro de migraciones `[(from, to, apply)]`.
    ///
    /// Returns:
    ///     Los pasos ordenados `from -> to`; vacío si ya está en la versión
    ///     actual.
    ///
    /// Raises:
    ///     [`RuscaError::CorruptManifest`] si el JSON es inválido.
    ///     [`RuscaError::SchemaVersionTooNew`] si la versión es futura.
    ///     [`RuscaError::MigrationFailed`] si la cadena de migraciones está
    ///     incompleta. `path` no se modifica en ningún caso.
    ///     [`RuscaError::Io`] si el fichero no se puede leer.
    pub fn migrate_plan(
        path: &Path,
        migrations: &[Migration],
    ) -> Result<Vec<PlannedMigration>, RuscaError> {
        let manifest = Self::read_validated(path)?;
        plan_migrations(manifest.schema_version, migrations, CURRENT_SCHEMA_VERSION)
    }

    /// Lee y valida estructuralmente el manifiesto (JSON + versión futura).
    ///
    /// Args:
    ///     path: Ruta del fichero `MANIFEST.json`.
    ///
    /// Returns:
    ///     El manifiesto deserializado con `schema_version <= CURRENT_SCHEMA_VERSION`.
    ///
    /// Raises:
    ///     [`RuscaError::CorruptManifest`] si el JSON es inválido o faltan campos.
    ///     [`RuscaError::SchemaVersionTooNew`] si la versión es más nueva que la
    ///     soportada.
    ///     [`RuscaError::Io`] si el fichero no se puede leer.
    fn read_validated(path: &Path) -> Result<Self, RuscaError> {
        let bytes = std::fs::read(path)?;
        let manifest: Self = serde_json::from_slice(&bytes).map_err(|error| {
            RuscaError::CorruptManifest(format!(
                "manifiesto inválido en {}: {error}",
                path.display()
            ))
        })?;
        if manifest.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(RuscaError::SchemaVersionTooNew {
                found: manifest.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        Ok(manifest)
    }

    /// Persiste el manifiesto de forma atómica.
    ///
    /// Escribe un fichero temporal hermano, fuerza `fsync` y lo renombra sobre
    /// `path`; el renombrado es atómico dentro del mismo sistema de ficheros.
    ///
    /// Args:
    ///     path: Ruta destino `MANIFEST.json`.
    ///
    /// Returns:
    ///     `Ok(())` cuando el manifiesto es durable.
    ///
    /// Raises:
    ///     [`RuscaError::CorruptManifest`] si la serialización falla.
    ///     [`RuscaError::Io`] si la escritura, el `fsync` o el `rename` fallan.
    pub fn store(&self, path: &Path) -> Result<(), RuscaError> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| {
            RuscaError::CorruptManifest(format!("no se pudo serializar el manifiesto: {error}"))
        })?;
        let tmp = temp_path(path);
        {
            let mut file = File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Incrementa la época lógica en uno.
    pub fn bump_epoch(&mut self) {
        self.epoch += 1;
    }
}

/// Deriva la ruta temporal hermana del manifiesto (mismo directorio y FS).
///
/// Args:
///     path: Ruta del manifiesto.
///
/// Returns:
///     La ruta `path` con el sufijo [`TMP_SUFFIX`] anexado.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(TMP_SUFFIX);
    PathBuf::from(name)
}
