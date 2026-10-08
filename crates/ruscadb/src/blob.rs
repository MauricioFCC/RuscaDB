//! Integración del blob store en la fachada (SPEC-0038, F4).
//!
//! [`Database`] delega en el [`BlobStore`](ruscadb_multimodal::BlobStore)
//! abierto con `DbConfig::blob_path`:
//!
//! - [`Database::put_blob`] / [`Database::get_blob`]: roundtrip content-addressed.
//! - [`Database::gc_blobs`]: GC con el **barrier R7** — solo se ejecuta si no
//!   hay una transacción en vuelo (`active_tx == None`).
//!
//! Sin `blob_path`, las tres operaciones devuelven un error accionable.

use ruscadb_core::RuscaError;
use ruscadb_multimodal::BlobHash;

use crate::database::Database;

/// Error accionable cuando la base se abrió sin blob store.
///
/// Returns:
///     [`RuscaError::InvalidConfig`] con la acción correctiva (fijar
///     `DbConfig::blob_path` o `Database::builder().blob_path(..)`).
fn blob_store_not_configured() -> RuscaError {
    RuscaError::InvalidConfig(
        "blob store no configurado: fija DbConfig::blob_path (o \
         Database::builder().blob_path(..)) al abrir la base para usar \
         put_blob/get_blob/gc_blobs"
            .to_string(),
    )
}

impl Database {
    /// Almacena `bytes` en el blob store y devuelve su hash de contenido.
    ///
    /// Args:
    ///     bytes: Contenido crudo del blob.
    ///
    /// Returns:
    ///     El [`BlobHash`] content-addressed (SHA-256) que identifica al
    ///     contenido; dos `put_blob` del mismo contenido devuelven el mismo hash.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si no hay blob store configurado;
    ///     [`RuscaError::Io`] si falla la escritura en disco.
    pub fn put_blob(&mut self, bytes: &[u8]) -> Result<BlobHash, RuscaError> {
        let store = self.blobs.as_mut().ok_or_else(blob_store_not_configured)?;
        store.put(bytes)
    }

    /// Lee el contenido completo del blob identificado por `hash`.
    ///
    /// Args:
    ///     hash: Hash del blob a leer.
    ///
    /// Returns:
    ///     Los bytes completos del blob.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si no hay blob store configurado;
    ///     [`RuscaError::Io`] si el blob no existe o falla la lectura.
    pub fn get_blob(&mut self, hash: &BlobHash) -> Result<Vec<u8>, RuscaError> {
        let store = self.blobs.as_ref().ok_or_else(blob_store_not_configured)?;
        store.get(hash)
    }

    /// Ejecuta el GC de blobs sin referencias vivas (barrier R7).
    ///
    /// Solo se permite con `active_tx == None` (sin transacciones en vuelo):
    /// con una transacción activa devuelve error accionable, porque un commit
    /// pendiente podría re-referenciar un blob recolectado.
    ///
    /// Returns:
    ///     El número de blobs eliminados (`0` si no había huérfanos).
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si hay una transacción activa o si no
    ///     hay blob store configurado;
    ///     [`RuscaError::Io`] si falla el recorrido del directorio o el borrado.
    pub fn gc_blobs(&mut self) -> Result<usize, RuscaError> {
        if self.active_tx.is_some() {
            return Err(RuscaError::InvalidConfig(
                "no se puede ejecutar gc_blobs con una transacción activa (barrier R7): \
                 confirma o aborta la transacción con commit()/rollback() antes del GC"
                    .to_string(),
            ));
        }
        let store = self.blobs.as_mut().ok_or_else(blob_store_not_configured)?;
        store.gc()
    }
}
