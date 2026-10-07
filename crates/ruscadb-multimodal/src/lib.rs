//! # ruscadb-multimodal
//!
//! Ingesta multimodal de RuscaDB: blobs content-addressed (CAS sha256 +
//! refcount) para imagen/audio/video/texto y orquestación del pipeline
//! blob → embedding → índice.
//!
//! Implementa el puerto `BlobStore` de `ruscadb-core`.
//! Diseño: `docs/RuscaDB-roadmap.md` §4.5 y §5. Fase: F4.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use ruscadb_core::RuscaError;
use sha2::{Digest, Sha256};

/// Subdirectorio, relativo a la raíz, donde viven los blobs.
const BLOBS_DIR: &str = "blobs";

/// Extensión de los ficheros de blob en disco.
const BLOB_EXTENSION: &str = "bin";

/// Ancho (en caracteres hexadecimales) de cada nivel de sharding.
const SHARD_WIDTH: usize = 2;

/// Ancho total del prefijo de sharding (`ab/cd`), igual a `SHARD_WIDTH * 2`.
const SHARD_PREFIX_WIDTH: usize = 4;

/// Hash de contenido SHA-256 en hexadecimal minúscula que identifica un blob.
///
/// Dos blobs con los mismos bytes comparten el mismo [`BlobHash`]
/// (deduplicación content-addressed).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlobHash(
    /// Digest SHA-256 en hexadecimal minúscula (64 caracteres).
    pub String,
);

impl fmt::Display for BlobHash {
    /// Escribe el digest hexadecimal del blob.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Almacén de blobs content-addressed con sharding `ab/cd/` y refcount.
///
/// El refcount vive en memoria (suficiente para esta fase): permite
/// deduplicar `put` y eliminar el fichero cuando el último consumidor hace
/// `unref`. Los blobs permanecen fuera del buffer pool (streaming directo).
#[derive(Debug)]
pub struct BlobStore {
    /// Raíz del almacén (contiene `blobs/`).
    root: PathBuf,
    /// Número de referencias vivas por hash, en memoria.
    refcounts: HashMap<BlobHash, u32>,
}

impl BlobStore {
    /// Abre (o crea) un blob store bajo `root`.
    ///
    /// Args:
    ///     root: Directorio raíz del almacén; se crea `root/blobs/` si falta.
    ///
    /// Returns:
    ///     El almacén listo para `put`/`get`.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si no se puede crear la jerarquía de directorios.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RuscaError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join(BLOBS_DIR))?;
        Ok(Self {
            root,
            refcounts: HashMap::new(),
        })
    }

    /// Almacena `bytes` y devuelve su hash de contenido (SHA-256).
    ///
    /// Args:
    ///     bytes: Contenido crudo del blob.
    ///
    /// Returns:
    ///     El [`BlobHash`] que identifica al contenido.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura en disco.
    ///
    /// La operación es idempotente a nivel de contenido: insertar dos veces
    /// los mismos bytes devuelve el mismo hash y solo escribe un fichero,
    /// incrementando el refcount.
    pub fn put(&mut self, bytes: &[u8]) -> Result<BlobHash, RuscaError> {
        let hash = digest(bytes);
        let path = self.blob_path(&hash);
        if !path.is_file() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, bytes)?;
        }
        let count = self.refcounts.entry(hash.clone()).or_insert(0);
        *count += 1;
        Ok(hash)
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
    ///     [`RuscaError::Io`] si el blob no existe o falla la lectura.
    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>, RuscaError> {
        Ok(fs::read(self.blob_path(hash))?)
    }

    /// Lee los bytes del rango `[start, end)` del blob.
    ///
    /// Args:
    ///     hash: Hash del blob a leer.
    ///     range: Rango semiabierto de bytes `[start, end)`.
    ///
    /// Returns:
    ///     Los bytes exactos del rango solicitado.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si el blob no existe o falla la lectura;
    ///     [`RuscaError::InvalidConfig`] si el rango excede el tamaño del blob.
    pub fn get_range(&self, hash: &BlobHash, range: Range<usize>) -> Result<Vec<u8>, RuscaError> {
        let bytes = self.get(hash)?;
        bytes.get(range).map(|slice| slice.to_vec()).ok_or_else(|| {
            RuscaError::InvalidConfig("rango fuera de los límites del blob".to_string())
        })
    }

    /// Consulta el número de referencias vivas de un blob.
    ///
    /// Args:
    ///     hash: Hash del blob.
    ///
    /// Returns:
    ///     `Some(refcount)` si el blob está registrado; `None` en caso contrario.
    pub fn ref_count(&self, hash: &BlobHash) -> Option<u32> {
        self.refcounts.get(hash).copied()
    }

    /// Libera una referencia al blob y lo borra si el contador llega a cero.
    ///
    /// Args:
    ///     hash: Hash del blob a liberar.
    ///
    /// Returns:
    ///     `Ok(())` al liberar la referencia.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el borrado del fichero.
    ///
    /// Si el blob no está registrado, la operación es un no-op idempotente.
    pub fn unref(&mut self, hash: &BlobHash) -> Result<(), RuscaError> {
        let Some(count) = self.refcounts.get_mut(hash) else {
            return Ok(());
        };
        if *count > 1 {
            *count -= 1;
            return Ok(());
        }
        self.refcounts.remove(hash);
        let path = self.blob_path(hash);
        if path.is_file() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Indica si el blob existe físicamente en el almacén.
    ///
    /// Args:
    ///     hash: Hash del blob.
    ///
    /// Returns:
    ///     `true` si el fichero del blob existe; `false` en caso contrario.
    pub fn contains(&self, hash: &BlobHash) -> bool {
        self.blob_path(hash).is_file()
    }

    /// Calcula la ruta en disco del blob, con sharding `ab/cd/<hash>.bin`.
    ///
    /// Args:
    ///     hash: Hash del blob.
    ///
    /// Returns:
    ///     La ruta absoluta dentro de `root/blobs/`.
    fn blob_path(&self, hash: &BlobHash) -> PathBuf {
        let first = hash.0.get(0..SHARD_WIDTH).unwrap_or("00");
        let second = hash.0.get(SHARD_WIDTH..SHARD_PREFIX_WIDTH).unwrap_or("00");
        self.root
            .join(BLOBS_DIR)
            .join(first)
            .join(second)
            .join(format!("{}.{}", hash.0, BLOB_EXTENSION))
    }
}

/// Calcula el hash SHA-256 de `bytes` en hexadecimal minúscula.
///
/// Args:
///     bytes: Contenido a hashear.
///
/// Returns:
///     El [`BlobHash`] content-addressed del contenido.
fn digest(bytes: &[u8]) -> BlobHash {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    BlobHash(hex::encode(hasher.finalize()))
}

/// Detecta el tipo MIME de un fichero a partir de su extensión.
///
/// Args:
///     path: Ruta (real o simulada) del fichero.
///
/// Returns:
///     El MIME correspondiente, o `application/octet-stream` si la extensión
///     es desconocida o ausente.
pub fn media_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("txt") => "text/plain",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// Cuenta recursivamente los ficheros `.bin` bajo el directorio de blobs.
    fn count_blobs(root: &Path) -> usize {
        fn walk(dir: &Path, count: &mut usize) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path, count);
                    } else if path.extension().and_then(|ext| ext.to_str()) == Some(BLOB_EXTENSION)
                    {
                        *count += 1;
                    }
                }
            }
        }
        let mut count = 0;
        walk(&root.join(BLOBS_DIR), &mut count);
        count
    }

    /// AC-0008-01 — el mismo contenido produce el mismo hash y refcount = 2.
    #[test] // @spec AC-0008-01
    fn test_ac_0008_01_put_is_content_addressed() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let mut store = BlobStore::open(dir.path()).expect("abrir store");
        let content = b"contenido deduplicado";

        let first = store.put(content).expect("primer put");
        let second = store.put(content).expect("segundo put");

        assert_eq!(first, second, "el mismo contenido debe dar el mismo hash");
        assert_eq!(store.ref_count(&first), Some(2));
        assert_eq!(count_blobs(dir.path()), 1, "solo un blob en disco");
    }

    /// AC-0008-02 — `get_range` devuelve exactamente los bytes del rango.
    #[test] // @spec AC-0008-02
    fn test_ac_0008_02_get_range_returns_bytes() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let mut store = BlobStore::open(dir.path()).expect("abrir store");
        let hash = store.put(b"0123456789abcdef").expect("put");

        let slice = store.get_range(&hash, 2..6).expect("rango válido");

        assert_eq!(slice, b"2345");
        assert_eq!(store.get(&hash).expect("get total"), b"0123456789abcdef");
    }

    /// AC-0008-03 — `unref` elimina el blob cuando el refcount llega a cero.
    #[test] // @spec AC-0008-03
    fn test_ac_0008_03_unref_removes_blob_at_zero() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let mut store = BlobStore::open(dir.path()).expect("abrir store");
        let hash = store.put(b"blob efimero").expect("put");

        assert_eq!(store.ref_count(&hash), Some(1));
        store.unref(&hash).expect("unref");
        assert_eq!(store.ref_count(&hash), None);
        assert!(!store.contains(&hash), "el blob debe desaparecer");
        assert_eq!(count_blobs(dir.path()), 0);
    }

    /// AC-0008-04 — el media_type se detecta por extensión.
    #[test] // @spec AC-0008-04
    fn test_ac_0008_04_media_type_detection() {
        let cases = [
            ("foto.jpg", "image/jpeg"),
            ("foto.jpeg", "image/jpeg"),
            ("foto.png", "image/png"),
            ("anim.gif", "image/gif"),
            ("clase.mp4", "video/mp4"),
            ("audio.mp3", "audio/mpeg"),
            ("audio.wav", "audio/wav"),
            ("notas.txt", "text/plain"),
            ("datos.json", "application/json"),
            ("binario.dat", "application/octet-stream"),
            ("sin_extension", "application/octet-stream"),
            ("MAYUS.PNG", "image/png"),
        ];
        for (name, expected) in cases {
            assert_eq!(media_type(Path::new(name)), expected, "archivo {name}");
        }
    }

    /// `open` crea la jerarquía y los blobs usan el layout `ab/cd/<hash>.bin`.
    #[test]
    fn test_open_creates_layout_and_blob_path() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let mut store = BlobStore::open(dir.path()).expect("abrir store");
        assert!(dir.path().join(BLOBS_DIR).is_dir(), "debe crear blobs/");

        let hash = store.put(b"layout").expect("put");
        let expected = dir
            .path()
            .join(BLOBS_DIR)
            .join(&hash.0[0..SHARD_WIDTH])
            .join(&hash.0[SHARD_WIDTH..SHARD_PREFIX_WIDTH])
            .join(format!("{}.{}", hash.0, BLOB_EXTENSION));
        assert!(
            expected.is_file(),
            "layout esperado: {}",
            expected.display()
        );
        assert_eq!(hash.to_string(), hash.0);
    }

    /// Contenidos distintos producen hashes distintos.
    #[test]
    fn test_distinct_content_yields_distinct_hash() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let mut store = BlobStore::open(dir.path()).expect("abrir store");
        let a = store.put(b"uno").expect("put uno");
        let b = store.put(b"dos").expect("put dos");
        assert_ne!(a, b);
        assert_eq!(count_blobs(dir.path()), 2);
    }

    /// Leer un blob inexistente o un rango fuera de límites es un error tipado.
    #[test]
    fn test_missing_blob_and_bad_range_are_errors() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let mut store = BlobStore::open(dir.path()).expect("abrir store");
        let missing = BlobHash("00".repeat(32));
        assert!(matches!(store.get(&missing), Err(RuscaError::Io(_))));
        assert!(matches!(
            store.get_range(&missing, 0..1),
            Err(RuscaError::Io(_))
        ));

        let hash = store.put(b"abc").expect("put");
        assert!(matches!(
            store.get_range(&hash, 1..99),
            Err(RuscaError::InvalidConfig(_))
        ));
        assert_eq!(store.ref_count(&missing), None);
        assert!(store.unref(&missing).is_ok(), "unref desconocido es no-op");
    }

    /// `unref` mantiene el blob mientras queden referencias.
    #[test]
    fn test_unref_keeps_blob_while_referenced() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let mut store = BlobStore::open(dir.path()).expect("abrir store");
        let hash = store.put(b"compartido").expect("put 1");
        let _ = store.put(b"compartido").expect("put 2");
        store.unref(&hash).expect("unref");
        assert_eq!(store.ref_count(&hash), Some(1));
        assert!(store.contains(&hash));
    }

    proptest! {
        /// Invariante I6 — `get(put(b)) == b` para cualquier contenido.
        #[test]
        fn prop_put_get_roundtrip(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let dir = tempfile::tempdir().expect("dir temporal");
            let mut store = BlobStore::open(dir.path()).expect("abrir store");
            let hash = store.put(&bytes).expect("put");
            let recovered = store.get(&hash).expect("get");
            prop_assert_eq!(recovered, bytes);
        }

        /// Invariante — `get_range` coincide con el slicing del contenido.
        #[test]
        fn prop_get_range_matches_slice(
            bytes in prop::collection::vec(any::<u8>(), 1..1024),
            start in 0usize..1024,
            len in 0usize..1024,
        ) {
            let start = start % bytes.len();
            let end = (start + len).min(bytes.len());
            let dir = tempfile::tempdir().expect("dir temporal");
            let mut store = BlobStore::open(dir.path()).expect("abrir store");
            let hash = store.put(&bytes).expect("put");
            let slice = store.get_range(&hash, start..end).expect("rango");
            prop_assert_eq!(slice, bytes[start..end].to_vec());
        }
    }
}
