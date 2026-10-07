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
    /// Args:
    ///     path: Ruta del fichero `MANIFEST.json`.
    ///
    /// Returns:
    ///     El manifiesto deserializado.
    ///
    /// Raises:
    ///     [`RuscaError::CorruptManifest`] si el JSON es inválido o faltan
    ///     campos obligatorios.
    ///     [`RuscaError::Io`] si el fichero no se puede leer.
    pub fn load(path: &Path) -> Result<Self, RuscaError> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(|error| {
            RuscaError::CorruptManifest(format!(
                "manifiesto inválido en {}: {error}",
                path.display()
            ))
        })
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
