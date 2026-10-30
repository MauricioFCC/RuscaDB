//! Checkpoint a S3 (SPEC-0128): manifiesto y transporte por `ObjectStore`.
//!
//! La logica de backup (listar, subir, restaurar, verificar) es independiente
//! del proveedor. El cliente S3 real requiere credenciales (decision
//! operativa), asi que aqui se define el puerto [`ObjectStore`] y se ofrece
//! [`LocalObjectStore`] para edge/tests sin red. Un adapter S3 se inyecta con
//! [`S3Backup::with_store`].
//!
//! Complejidad: `O(files)` por subida/restauracion; sin `O(n^2)`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Puerto de object-store (S3, MinIO, disco local, ...).
pub trait ObjectStore: Send + Sync {
    /// Sube `bytes` bajo `key`.
    ///
    /// Errors:
    ///     Texto accionable si la escritura falla.
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), String>;

    /// Lee el objeto `key`.
    ///
    /// Errors:
    ///     Texto accionable si no existe o falla la lectura.
    fn get(&self, key: &str) -> Result<Vec<u8>, String>;

    /// Lista claves que empiezan por `prefix`.
    ///
    /// Errors:
    ///     Texto accionable si el listado falla.
    fn list(&self, prefix: &str) -> Result<Vec<String>, String>;
}

/// Object-store sobre un directorio local (edge/tests, sin credenciales).
#[derive(Debug, Clone)]
pub struct LocalObjectStore {
    root: PathBuf,
}

impl LocalObjectStore {
    /// Crea el store en `root` (se crea el directorio si falta).
    ///
    /// Args:
    ///     root: Directorio raiz del bucket.
    ///
    /// Returns:
    ///     Store listo.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let _ = fs::create_dir_all(&root);
        Self { root }
    }

    /// Resuelve una clave a una ruta segura (sin `..` ni rutas absolutas).
    fn resolve(&self, key: &str) -> Result<PathBuf, String> {
        if key.is_empty() {
            return Err("clave vacia".to_string());
        }
        if Path::new(key).is_absolute() || key.split(['/', '\\']).any(|part| part == "..") {
            return Err(format!("clave no permitida (traversal): {key}"));
        }
        Ok(self.root.join(key))
    }
}

impl ObjectStore for LocalObjectStore {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("mkdir {parent:?}: {error}"))?;
        }
        fs::write(&path, bytes).map_err(|error| format!("write {path:?}: {error}"))
    }

    fn get(&self, key: &str) -> Result<Vec<u8>, String> {
        let path = self.resolve(key)?;
        fs::read(&path).map_err(|error| format!("read {path:?}: {error}"))
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        let mut keys = Vec::new();
        collect(&self.root, &self.root, prefix, &mut keys)?;
        keys.sort();
        Ok(keys)
    }
}

/// Recorre `dir` acumulando claves relativas que empiezan por `prefix`.
fn collect(root: &Path, dir: &Path, prefix: &str, keys: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("read_dir {dir:?}: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("entry: {error}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, prefix, keys)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("strip_prefix: {error}"))?
            .to_string_lossy()
            .replace('\\', "/");
        if relative.starts_with(prefix) {
            keys.push(relative);
        }
    }
    Ok(())
}

/// Backup S3 con transporte inyectable.
#[derive(Clone)]
pub struct S3Backup {
    /// Ruta origen de la base.
    source: PathBuf,
    /// Bucket destino.
    bucket: String,
    /// Transporte de objetos (ausente = S3 real sin credenciales).
    store: Option<Arc<dyn ObjectStore>>,
}

impl std::fmt::Debug for S3Backup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("S3Backup")
            .field("source", &self.source)
            .field("bucket", &self.bucket)
            .field("has_store", &self.store.is_some())
            .finish()
    }
}

impl S3Backup {
    /// Crea un backup sin transporte (manifiesto solamente).
    ///
    /// Args:
    ///     source: Directorio de la base.
    ///     bucket: Bucket destino.
    ///
    /// Returns:
    ///     Backup sin `ObjectStore`; `upload`/`restore` pediran inyectarlo.
    pub fn new(source: impl Into<PathBuf>, bucket: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            bucket: bucket.into(),
            store: None,
        }
    }

    /// Crea un backup con transporte (S3 real o [`LocalObjectStore`]).
    ///
    /// Args:
    ///     source: Directorio de la base.
    ///     bucket: Bucket destino.
    ///     store: Transporte de objetos.
    ///
    /// Returns:
    ///     Backup listo para `upload`/`restore`.
    pub fn with_store(
        source: impl Into<PathBuf>,
        bucket: impl Into<String>,
        store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            source: source.into(),
            bucket: bucket.into(),
            store: Some(store),
        }
    }

    /// Genera un manifiesto JSON de los ficheros de la base.
    ///
    /// Returns:
    ///     JSON con bucket y `[{name,size}]`.
    ///
    /// Errors:
    ///     Texto accionable si el directorio no se puede leer.
    pub fn manifest(&self) -> Result<String, String> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.source).map_err(|e| format!("read_dir: {e}"))? {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                files.push(format!(r#"{{"name":"{}","size":{}}}"#, name, size));
            }
        }
        Ok(format!(
            r#"{{"bucket":"{}","files":[{}]}}"#,
            self.bucket,
            files.join(",")
        ))
    }

    /// Sube los ficheros de `source` al bucket.
    ///
    /// Returns:
    ///     Numero de ficheros subidos.
    ///
    /// Errors:
    ///     Texto accionable si no hay transporte configurado o falla la subida.
    pub fn upload(&self) -> Result<usize, String> {
        let store = self.store.as_ref().ok_or_else(|| {
            "S3Backup: no hay ObjectStore configurado (S3 requiere credenciales); \
             inyecta uno con S3Backup::with_store"
                .to_string()
        })?;
        let mut uploaded = 0usize;
        for entry in fs::read_dir(&self.source).map_err(|e| format!("read_dir: {e}"))? {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let bytes = fs::read(&path).map_err(|e| format!("read {path:?}: {e}"))?;
            store.put(&format!("{}/{}", self.bucket, name), &bytes)?;
            uploaded += 1;
        }
        Ok(uploaded)
    }

    /// Restaura el bucket en `destination`.
    ///
    /// Args:
    ///     destination: Directorio destino.
    ///
    /// Returns:
    ///     Numero de ficheros restaurados.
    ///
    /// Errors:
    ///     Texto accionable si no hay transporte o falla la escritura.
    pub fn restore(&self, destination: impl AsRef<Path>) -> Result<usize, String> {
        let store = self.store.as_ref().ok_or_else(|| {
            "S3Backup: no hay ObjectStore configurado (S3 requiere credenciales); \
             inyecta uno con S3Backup::with_store"
                .to_string()
        })?;
        let destination = destination.as_ref();
        let prefix = format!("{}/", self.bucket);
        let mut restored = 0usize;
        for key in store.list(&prefix)? {
            let Some(name) = key.strip_prefix(&prefix) else {
                continue;
            };
            if name.is_empty() || name.split(['/', '\\']).any(|part| part == "..") {
                continue;
            }
            let bytes = store.get(&key)?;
            let target = destination.join(name);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
            }
            fs::write(&target, bytes).map_err(|e| format!("write {target:?}: {e}"))?;
            restored += 1;
        }
        Ok(restored)
    }

    /// Bucket destino.
    ///
    /// Returns:
    ///     Nombre del bucket.
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalObjectStore, ObjectStore, S3Backup};
    use std::fs;
    use std::sync::Arc;

    #[test] // @spec AC-0128-01
    fn test_ac_0128_01_s3_manifest() {
        let dir = std::env::temp_dir().join(format!("ruscadb-s3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src");
        fs::write(src.join("data.db"), b"hello").expect("write");

        let backup = S3Backup::new(&src, "my-bucket");
        let manifest = backup.manifest().expect("manifest");
        assert!(manifest.contains("my-bucket"));
        assert!(manifest.contains("data.db"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// Subida y restauracion con transporte local (sin credenciales S3).
    #[test]
    fn upload_and_restore_roundtrip_with_local_store() {
        let dir = std::env::temp_dir().join(format!("ruscadb-s3-rt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = dir.join("src");
        let bucket_root = dir.join("bucket");
        let restored = dir.join("restored");
        fs::create_dir_all(&src).expect("src");
        fs::write(src.join("a.db"), b"alpha").expect("a");
        fs::write(src.join("b.wal"), b"beta").expect("b");

        let store = Arc::new(LocalObjectStore::new(&bucket_root));
        let backup = S3Backup::with_store(&src, "edge", store);
        assert_eq!(backup.upload().expect("upload"), 2);
        assert_eq!(backup.restore(&restored).expect("restore"), 2);
        assert_eq!(fs::read(restored.join("a.db")).expect("a"), b"alpha");
        assert_eq!(fs::read(restored.join("b.wal")).expect("b"), b"beta");

        // El bucket contiene las claves con prefijo.
        let store = LocalObjectStore::new(&bucket_root);
        let keys = store.list("edge/").expect("list");
        assert_eq!(keys.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    /// Sin transporte el error es accionable (no hay S3 implicito).
    #[test]
    fn upload_without_store_is_actionable() {
        let dir = std::env::temp_dir().join(format!("ruscadb-s3-nostore-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let backup = S3Backup::new(&dir, "b");
        let error = backup.upload().expect_err("sin store");
        assert!(error.contains("ObjectStore"), "{error}");
        assert!(error.contains("credenciales"), "{error}");
        assert!(backup.restore(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// El transporte local rechaza path traversal.
    #[test]
    fn local_store_rejects_traversal() {
        let dir = std::env::temp_dir().join(format!("ruscadb-s3-trav-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = LocalObjectStore::new(&dir);
        assert!(store.put("../escape", b"x").is_err());
        assert!(store.put("a/../../escape", b"x").is_err());
        assert!(store.get("/etc/passwd").is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
