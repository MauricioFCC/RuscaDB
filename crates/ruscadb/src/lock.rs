//! Lock cross-proceso de la base (SPEC-0076).
//!
//! [`LockGuard`] mantiene un lock exclusivo **no-bloqueante** sobre un sidecar
//! `<data>.lock` mientras la [`crate::Database`] viva. Se usa `std::fs::File`
//! (`File::try_lock`, estable desde Rust 1.89): en POSIX `flock`, en Windows
//! `LockFileEx`. El lock **nunca** se toma sobre el fichero de datos, porque en
//! Windows `LockFileEx` es *mandatory* y bloquearía los reads/mmap del `.data`.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use ruscadb_core::RuscaError;

/// Guard RAII del lock cross-proceso sobre el sidecar `<data>.lock`.
///
/// Adquiere el lock en [`LockGuard::acquire`] y lo libera al dropearse: cerrar
/// el descriptor libera el lock del sistema operativo (POSIX y Windows).
pub(crate) struct LockGuard {
    /// Descriptor del sidecar; cerrarlo libera el lock del sistema operativo.
    file: File,
}

impl LockGuard {
    /// Adquiere el lock exclusivo no-bloqueante sobre `<data>.lock`.
    ///
    /// Abre (o crea, sin truncar) el sidecar y hace `try_lock()`. El lock se
    /// toma **sobre el sidecar**, nunca sobre el fichero de datos: así se evita
    /// el `LockFileEx` obligatorio de Windows, que bloquearía las lecturas y el
    /// `mmap` del `.data`.
    ///
    /// Args:
    ///     data_path: Ruta del fichero de datos de la base; el sidecar es
    ///         `data_path.with_extension("lock")`.
    ///
    /// Returns:
    ///     El guard que mantiene el lock hasta su `Drop`.
    ///
    /// Errors:
    ///     [`RuscaError::DatabaseInUse`] si otra instancia ya tiene el lock
    ///     (WHAT: la base está en uso; WHY: el lock exclusivo lo tiene otro
    ///     handle/proceso; WHERE: la ruta del fichero de datos);
    ///     [`RuscaError::Io`] si no se puede abrir el sidecar.
    pub(crate) fn acquire(data_path: &Path) -> Result<Self, RuscaError> {
        let sidecar = data_path.with_extension("lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&sidecar)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(TryLockError::WouldBlock) => Err(RuscaError::DatabaseInUse {
                path: data_path.display().to_string(),
            }),
            Err(TryLockError::Error(error)) => Err(RuscaError::Io(error)),
        }
    }
}

impl Drop for LockGuard {
    /// Libera el lock del sistema operativo sin panics.
    ///
    /// Si `unlock` falla, cerrar el descriptor en el `Drop` del `File` también
    /// lo libera; un fallo aquí no es accionable ni recuperable, así que se
    /// descarta explícitamente (sin `unwrap`/`expect`).
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
