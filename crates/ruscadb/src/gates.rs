//! Gates de ingesta + registry persistible (SPEC-0064).
//!
//! - [`IngestionPolicy`]: configuración por instancia (tope de bytes por
//!   registro y exigencia de modelo registrado).
//! - Persistencia del [`ModelRegistry`] en `<data>.models.json`: se guarda al
//!   registrar un modelo y se restaura al abrir la base.

use std::path::Path;

use ruscadb_core::{Metric, Record, RuscaError};

use crate::database::Database;

/// Funcion reservada para el nombre de archivo del registry de modelos.
const MODELS_FILE_EXTENSION: &str = "models.json";

/// Política de ingesta configurable por instancia (SPEC-0064).
///
/// Por defecto está **abierta** (`max_record_bytes = 0` sin límite,
/// `require_registry = false`): cero regresión con bases existentes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestionPolicy {
    /// Tope de bytes del `Record` serializado (postcard); `0` = sin límite.
    pub max_record_bytes: usize,
    /// `true` → el registro de modelos no puede estar vacío al insertar
    /// registros con vector (aplica además la allowlist de SPEC-0032).
    pub require_registry: bool,
}

impl Default for IngestionPolicy {
    /// Política abierta: sin límite de tamaño y sin exigencia de registry.
    fn default() -> Self {
        Self {
            max_record_bytes: 0,
            require_registry: false,
        }
    }
}

impl Database {
    /// Fija la política de ingesta de la instancia.
    ///
    /// Args:
    ///     policy: Nueva política (tamaño máximo y exigencia de registry).
    pub fn set_ingestion_policy(&mut self, policy: IngestionPolicy) {
        self.policy = policy;
    }

    /// Devuelve la política de ingesta vigente.
    ///
    /// Returns:
    ///     La política configurada (por defecto, abierta).
    pub fn ingestion_policy(&self) -> IngestionPolicy {
        self.policy
    }

    /// Aplica los gates de ingesta a un registro antes de insertarlo.
    ///
    /// 1. Tamaño: si `max_record_bytes > 0`, serializa el `Record` con postcard
    ///    y rechaza con [`RuscaError::ResourceLimit`] citando el tope.
    /// 2. Registry: si `require_registry` y el registro trae vector, exige que
    ///    el registro de modelos NO esté vacío; la allowlist de SPEC-0032
    ///    (`apply_registry`) se encarga del resto.
    ///
    /// Args:
    ///     table: Tabla destino (para mensajes accionables).
    ///     record: Registro a validar.
    ///
    /// Errors:
    ///     [`RuscaError::ResourceLimit`] si el registro supera el tope;
    ///     [`RuscaError::InvalidConfig`] si `require_registry` y no hay modelos.
    pub(crate) fn enforce_ingestion_gates(
        &self,
        table: &str,
        record: &Record,
    ) -> Result<(), RuscaError> {
        if self.policy.max_record_bytes > 0 {
            let size = postcard::to_allocvec(record)
                .map_err(|_| {
                    RuscaError::InvalidConfig(format!(
                        "no se puede serializar el registro para el gate de ingesta en '{table}'"
                    ))
                })?
                .len();
            if size > self.policy.max_record_bytes {
                return Err(RuscaError::ResourceLimit {
                    resource: "record_bytes".to_string(),
                    limit: self.policy.max_record_bytes as u64,
                    actual: size as u64,
                });
            }
        }
        if self.policy.require_registry && record.vector.is_some() && self.registry.is_empty() {
            return Err(RuscaError::InvalidConfig(
                "require_registry activo pero el registro de modelos está vacío: registra al \
                 menos un modelo antes de insertar vectores"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Persiste el registro de modelos en `<data>.models.json`.
    ///
    /// Serieliza las specs como JSON (dim, métrica y versión) de forma
    /// atómica (write a temporal + rename).
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura.
    pub(crate) fn persist_registry(&self) -> Result<(), RuscaError> {
        let entries = self.registry.entries();
        let payload: Vec<serde_json::Value> = entries
            .into_iter()
            .map(|(model_id, spec)| {
                serde_json::json!({
                    "model_id": model_id,
                    "dim": spec.dim,
                    "metric": spec.metric,
                    "version": spec.version,
                })
            })
            .collect();
        let bytes = serde_json::to_vec_pretty(&payload).map_err(|error| {
            RuscaError::InvalidConfig(format!("registry no serializable: {error}"))
        })?;
        let path = models_path_for(self.models_path());
        atomic_write(&path, &bytes)?;
        Ok(())
    }

    /// Restaura el registro de modelos persistido al abrir la base.
    ///
    /// Lee `<data>.models.json` si existe y registra cada spec; un fichero
    /// ausente no es un error (bases sin registry).
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la lectura;
    ///     [`RuscaError::InvalidConfig`] si el JSON es inválido.
    pub(crate) fn restore_registry(&mut self) -> Result<(), RuscaError> {
        let path = models_path_for(self.models_path());
        if !path.exists() {
            return Ok(());
        }
        let bytes = std::fs::read(&path)?;
        let entries: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
            .map_err(|error| RuscaError::InvalidConfig(format!("registry inválido: {error}")))?;
        for entry in entries {
            let model_id = entry
                .get("model_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    RuscaError::InvalidConfig("registry: entrada sin model_id".to_string())
                })?;
            let dim = entry
                .get("dim")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    RuscaError::InvalidConfig(format!("registry: '{model_id}' sin dim"))
                })? as usize;
            let metric: Metric =
                serde_json::from_value(entry.get("metric").cloned().ok_or_else(|| {
                    RuscaError::InvalidConfig(format!("registry: '{model_id}' sin metric"))
                })?)
                .map_err(|error| {
                    RuscaError::InvalidConfig(format!(
                        "registry: '{model_id}' métrica inválida: {error}"
                    ))
                })?;
            let version = entry
                .get("version")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    RuscaError::InvalidConfig(format!("registry: '{model_id}' sin version"))
                })? as u32;
            self.registry.register(model_id, dim, metric, version)?;
        }
        Ok(())
    }

    /// Devuelve la ruta base del fichero de registry (para derivar lógicos).
    ///
    /// Returns:
    ///     La ruta `<data>.models.json` vigente.
    pub(crate) fn models_path(&self) -> &Path {
        &self.models_path
    }
}

/// Derivar la ruta del registry de modelos a partir del archivo de datos.
///
/// Args:
///     data_path: Ruta del `PagedFile` (`.db`).
///
/// Returns:
///     La ruta con extensión `models.json`.
fn models_path_for(data_path: &Path) -> std::path::PathBuf {
    data_path.with_extension(MODELS_FILE_EXTENSION)
}

/// Escribe un fichero de forma atómica (temporal + rename).
///
/// Args:
///     path: Destino final.
///     bytes: Contenido a escribir.
///
/// Errors:
///     [`RuscaError::Io`] si falla la escritura o el rename.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RuscaError> {
    let tmp = path.with_extension("models.json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
