//! # ruscadb-multimodal
//!
//! Ingesta multimodal de RuscaDB: blobs content-addressed (CAS sha256 +
//! refcount) para imagen/audio/video/texto y orquestación del pipeline
//! blob → embedding → índice.
//!
//! Implementa el puerto `BlobStore` de `ruscadb-core`.
//! Diseño: `docs/RuscaDB-roadmap.md` §4.5 y §5. Fase: F4.
//!
//! ## Cifrado en reposo (SPEC-0013)
//!
//! En modo cifrado cada blob se guarda como
//! `[RCE1 | 0x01 | nonce 24 B | seal(bytes)]` (XChaCha20-Poly1305 vía
//! `ruscadb-crypto`):
//!
//! - **Nonce determinista sin RNG**: primeros 24 B del SHA-256 del contenido.
//!   Igualdad de contenido ⇒ igualdad de nonce ⇒ igualdad de ciphertext (el
//!   AEAD es determinista), luego la deduplicación CAS se preserva byte a byte.
//! - **Modo estricto por raíz**: una raíz se usa en un solo modo. Leer un blob
//!   claro desde un store cifrado (o viceversa) es error tipado, nunca basura
//!   silenciosa; el magic `RCE1` distingue ambos formatos (un solo byte de
//!   versión colisionaría con contenido claro, p. ej. blobs que empiezan por
//!   `0x01`).
//! - La clave vive solo en memoria y se borra en [`Drop`].

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::Read;
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

/// Magic del envelope cifrado `[RCE1 | 0x01 | nonce 24 B | seal(bytes)]`.
///
/// Solo el magic completo acredita el formato: un byte de versión colisiona
/// con contenido claro legítimo.
const ENCRYPTED_MAGIC: [u8; 4] = *b"RCE1";
/// Byte de versión del envelope cifrado (se valida explícitamente: el AEAD no
/// lo cubre y un flip debe rechazarse, no silenciarse).
const ENCRYPTED_VERSION: u8 = 0x01;
/// Cabecera del envelope: magic (4) + versión (1).
const ENVELOPE_HEADER: usize = 4 + 1;
/// Tamaño del tag Poly1305 anexado por `ruscadb_crypto::seal` (16 B).
const AEAD_TAG_SIZE: usize = 16;
/// Sobrecoste del envelope sobre el claro: cabecera (5) + nonce (24) + tag (16).
///
/// Es también la longitud mínima de un envelope válido (claro vacío).
const ENVELOPE_OVERHEAD: usize = ENVELOPE_HEADER + ruscadb_crypto::NONCE_SIZE + AEAD_TAG_SIZE;

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
pub struct BlobStore {
    /// Raíz del almacén (contiene `blobs/`).
    root: PathBuf,
    /// Número de referencias vivas por hash, en memoria.
    refcounts: HashMap<BlobHash, u32>,
    /// Clave AEAD opcional (`None` = modo claro). Se borra en [`Drop`].
    key: Option<[u8; 32]>,
}

impl std::fmt::Debug for BlobStore {
    /// Formato sin exponer la clave (solo indica si hay cifrado).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobStore")
            .field("root", &self.root)
            .field("encrypted", &self.key.is_some())
            .finish()
    }
}

impl Drop for BlobStore {
    /// Borra el material de clave por sobreescritura (best-effort sin `zeroize`).
    fn drop(&mut self) {
        if let Some(key) = self.key.as_mut() {
            for byte in key.iter_mut() {
                *byte = 0;
            }
        }
    }
}

/// Apertura compartida de `BlobStore::open` y `BlobStore::open_encrypted`.
///
/// Args:
///     root: Directorio raíz del almacén.
///     key: Clave AEAD (`None` = modo claro).
///
/// Returns:
///     El almacén listo para `put`/`get`.
///
/// Raises:
///     [`RuscaError::Io`] si no se puede crear la jerarquía de directorios.
fn open_impl(root: impl AsRef<Path>, key: Option<[u8; 32]>) -> Result<BlobStore, RuscaError> {
    let root = root.as_ref().to_path_buf();
    fs::create_dir_all(root.join(BLOBS_DIR))?;
    Ok(BlobStore {
        root,
        refcounts: HashMap::new(),
        key,
    })
}

/// Formato de un blob tal como está en disco (por magic de envelope).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoredKind {
    /// Contenido en claro (o fichero vacío).
    Clear,
    /// Envelope `[RCE1 | 0x01 | nonce | seal]` (longitud plausible).
    Encrypted,
    /// Magic `RCE1` con longitud menor que el envelope mínimo.
    Truncated,
}

/// Clasifica bytes ya leídos por su magic de envelope.
///
/// Args:
///     stored: Contenido íntegro del fichero del blob.
///
/// Returns:
///     El formato detectado (sin validar el AEAD).
fn classify_bytes(stored: &[u8]) -> StoredKind {
    if !stored.starts_with(&ENCRYPTED_MAGIC) {
        return StoredKind::Clear;
    }
    if stored.len() < ENVELOPE_OVERHEAD {
        return StoredKind::Truncated;
    }
    StoredKind::Encrypted
}

/// Clasifica el fichero `path` leyendo solo metadatos + magic.
///
/// Evita leer blobs grandes en `put` cuando el fichero ya existe (dedup).
///
/// Args:
///     path: Ruta del fichero del blob.
///
/// Returns:
///     El formato detectado (sin validar el AEAD).
///
/// Raises:
///     [`RuscaError::Io`] si falla el acceso a disco.
fn stored_kind(path: &Path) -> Result<StoredKind, RuscaError> {
    let len = fs::metadata(path)?.len();
    if len == 0 {
        return Ok(StoredKind::Clear);
    }
    let mut head = [0u8; 4];
    let mut file = fs::File::open(path)?;
    let mut read = 0usize;
    while read < head.len() {
        let count = file.read(&mut head[read..])?;
        if count == 0 {
            break;
        }
        read += count;
    }
    if head[..read] != ENCRYPTED_MAGIC[..read] {
        return Ok(StoredKind::Clear);
    }
    if len < ENVELOPE_OVERHEAD as u64 {
        return Ok(StoredKind::Truncated);
    }
    Ok(StoredKind::Encrypted)
}

/// Deriva el nonce determinista del envelope (primeros 24 B del SHA-256).
///
/// Args:
///     bytes: Contenido en claro del blob.
///
/// Returns:
///     El nonce para `ruscadb_crypto::{seal, open}`.
fn content_nonce(bytes: &[u8]) -> [u8; ruscadb_crypto::NONCE_SIZE] {
    let digest = Sha256::digest(bytes);
    let mut nonce = [0u8; ruscadb_crypto::NONCE_SIZE];
    nonce.copy_from_slice(&digest[..ruscadb_crypto::NONCE_SIZE]);
    nonce
}

/// Envuelve `bytes` en `[RCE1 | 0x01 | nonce 24 B | seal(bytes)]`.
///
/// Args:
///     key: Clave simétrica de 32 B.
///     bytes: Contenido en claro.
///
/// Returns:
///     El envelope listo para persistir.
fn seal_blob(key: &[u8; 32], bytes: &[u8]) -> Vec<u8> {
    let nonce = content_nonce(bytes);
    let ciphertext = ruscadb_crypto::seal(key, &nonce, bytes);
    let mut envelope = Vec::with_capacity(ENVELOPE_OVERHEAD + bytes.len());
    envelope.extend_from_slice(&ENCRYPTED_MAGIC);
    envelope.push(ENCRYPTED_VERSION);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    envelope
}

/// Extrae el `(nonce, ciphertext)` de un envelope versionado.
///
/// Args:
///     stored: Contenido íntegro del fichero (formato [`StoredKind::Encrypted`]).
///
/// Returns:
///     El nonce almacenado y el ciphertext sellado.
///
/// Raises:
///     [`RuscaError::InvalidConfig`] si el envelope está truncado o su
///     versión no es soportada.
fn parse_blob_envelope(
    stored: &[u8],
) -> Result<([u8; ruscadb_crypto::NONCE_SIZE], &[u8]), RuscaError> {
    if stored.get(4) != Some(&ENCRYPTED_VERSION) {
        return Err(RuscaError::InvalidConfig(format!(
            "versión de envelope no soportada: {:02x?}",
            stored.get(4)
        )));
    }
    let nonce_bytes = stored
        .get(ENVELOPE_HEADER..ENVELOPE_HEADER + ruscadb_crypto::NONCE_SIZE)
        .filter(|_| stored.len() >= ENVELOPE_OVERHEAD)
        .ok_or_else(|| {
            RuscaError::InvalidConfig(format!(
                "envelope de blob truncado ({} bytes, mínimo {})",
                stored.len(),
                ENVELOPE_OVERHEAD
            ))
        })?;
    let mut nonce = [0u8; ruscadb_crypto::NONCE_SIZE];
    nonce.copy_from_slice(nonce_bytes);
    Ok((
        nonce,
        &stored[ENVELOPE_HEADER + ruscadb_crypto::NONCE_SIZE..],
    ))
}

/// Devuelve el contenido en claro aplicando el modo del store.
///
/// Reglas de modo estricto: el formato en disco debe coincidir con el modo del
/// store; cualquier desvío (incluido el envelope truncado) es
/// [`RuscaError::InvalidConfig`], nunca basura silenciosa. El AEAD inválido
/// (clave errónea o manipulación) lo rechaza `ruscadb_crypto::open`.
///
/// Args:
///     key: Clave AEAD del store (`None` = modo claro).
///     stored: Contenido íntegro del fichero.
///
/// Returns:
///     El contenido en claro del blob.
///
/// Raises:
///     [`RuscaError::InvalidConfig`] ante mezcla de modos, envelope truncado
///     o AEAD inválido.
fn decrypt_blob(key: Option<&[u8; 32]>, stored: &[u8]) -> Result<Vec<u8>, RuscaError> {
    match (key, classify_bytes(stored)) {
        (None, StoredKind::Clear) => Ok(stored.to_vec()),
        (None, _) => Err(RuscaError::InvalidConfig(
            "blob con magic RCE1 (cifrado) en store claro: abra con open_encrypted".to_string(),
        )),
        (Some(_), StoredKind::Clear) => Err(RuscaError::InvalidConfig(
            "blob en claro dentro de un store cifrado (mezcla de modos)".to_string(),
        )),
        (Some(_), StoredKind::Truncated) => Err(RuscaError::InvalidConfig(
            "envelope de blob truncado en store cifrado".to_string(),
        )),
        (Some(key), StoredKind::Encrypted) => {
            let (nonce, ciphertext) = parse_blob_envelope(stored)?;
            ruscadb_crypto::open(key, &nonce, ciphertext)
        }
    }
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
        open_impl(root, None)
    }

    /// Abre (o crea) un blob store cifrado bajo `root` (SPEC-0013, AC-0013-03).
    ///
    /// Cada blob se persiste como `[RCE1 | 0x01 | nonce 24 B | seal(bytes)]` con nonce
    /// = primeros 24 B del SHA-256 del contenido: determinista, preserva la
    /// deduplicación CAS (mismo contenido ⇒ mismo ciphertext). La clave se
    /// borra en [`Drop`].
    ///
    /// Args:
    ///     root: Directorio raíz del almacén; se crea `root/blobs/` si falta.
    ///     key: Clave simétrica de 32 B (XChaCha20-Poly1305 vía `ruscadb-crypto`).
    ///
    /// Returns:
    ///     El almacén listo para `put`/`get` cifrados.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si no se puede crear la jerarquía de directorios.
    pub fn open_encrypted(root: impl AsRef<Path>, key: &[u8; 32]) -> Result<Self, RuscaError> {
        open_impl(root, Some(*key))
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
    ///     [`RuscaError::Io`] si falla la escritura en disco;
    ///     [`RuscaError::InvalidConfig`] si el fichero existente es de otro
    ///     modo (mezcla de modos).
    ///
    /// La operación es idempotente a nivel de contenido: insertar dos veces
    /// los mismos bytes devuelve el mismo hash y solo escribe un fichero,
    /// incrementando el refcount. En modo cifrado el hash se calcula sobre el
    /// claro y el envelope es determinista, luego el dedup se preserva.
    pub fn put(&mut self, bytes: &[u8]) -> Result<BlobHash, RuscaError> {
        let hash = digest(bytes);
        let path = self.blob_path(&hash);
        if path.is_file() {
            self.ensure_mode(&path)?;
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let stored = match self.key.as_ref() {
                None => bytes.to_vec(),
                Some(key) => seal_blob(key, bytes),
            };
            fs::write(&path, stored)?;
        }
        let count = self.refcounts.entry(hash.clone()).or_insert(0);
        *count += 1;
        Ok(hash)
    }

    /// Verifica que el fichero existente coincida con el modo del store.
    ///
    /// Args:
    ///     path: Ruta del fichero del blob ya existente.
    ///
    /// Returns:
    ///     `Ok(())` si el formato en disco coincide con el modo del store.
    ///
    /// Raises:
    ///     [`RuscaError::InvalidConfig`] ante mezcla de modos o envelope
    ///     truncado; [`RuscaError::Io`] si falla el acceso a disco.
    fn ensure_mode(&self, path: &Path) -> Result<(), RuscaError> {
        let kind = stored_kind(path)?;
        let encrypted = self.key.is_some();
        let matches =
            kind == StoredKind::Encrypted && encrypted || kind == StoredKind::Clear && !encrypted;
        if matches {
            return Ok(());
        }
        Err(RuscaError::InvalidConfig(format!(
            "blob en disco ({kind:?}) incompatible con el modo del store (cifrado: {encrypted})"
        )))
    }

    /// Lee el contenido completo del blob identificado por `hash`.
    ///
    /// En modo cifrado descifra el envelope antes de devolver los bytes.
    ///
    /// Args:
    ///     hash: Hash del blob a leer.
    ///
    /// Returns:
    ///     Los bytes completos del blob en claro.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si el blob no existe o falla la lectura;
    ///     [`RuscaError::InvalidConfig`] ante mezcla de modos, envelope
    ///     truncado o AEAD inválido (clave errónea o manipulación).
    pub fn get(&self, hash: &BlobHash) -> Result<Vec<u8>, RuscaError> {
        let stored = fs::read(self.blob_path(hash))?;
        decrypt_blob(self.key.as_ref(), &stored)
    }

    /// Lee los bytes del rango `[start, end)` del blob.
    ///
    /// En modo cifrado el rango se aplica sobre el claro ya descifrado.
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
    ///     [`RuscaError::InvalidConfig`] si el rango excede el tamaño del blob,
    ///     si hay mezcla de modos o si el AEAD no verifica.
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

    /// Clave de prueba fija (solo tests; en producción viene de KDF/entorno).
    fn test_key() -> [u8; 32] {
        [0x2au8; 32]
    }

    /// Lee los bytes crudos del fichero del blob (para inspeccionar el envelope).
    fn raw_blob_bytes(root: &Path, hash: &BlobHash) -> Vec<u8> {
        let first = hash.0.get(0..SHARD_WIDTH).unwrap_or("00");
        let second = hash.0.get(SHARD_WIDTH..SHARD_PREFIX_WIDTH).unwrap_or("00");
        let path = root
            .join(BLOBS_DIR)
            .join(first)
            .join(second)
            .join(format!("{hash}.{BLOB_EXTENSION}"));
        fs::read(path).expect("lee crudo")
    }

    /// AC-0013-03 — roundtrip cifrado de blobs con reapertura.
    ///
    /// Verifica `put`/`get`/`get_range`, refcount, reapertura con la misma
    /// clave, ausencia de claro en disco, dedup determinista y que abrir la
    /// misma raíz en claro es error (nunca basura silenciosa).
    #[test] // @spec AC-0013-03
    fn test_ac_0013_03_encrypted_blob_roundtrip() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let key = test_key();
        let contents: [&[u8]; 4] = [b"blob secreto 0013", b"", b"\x01lider", &[0xabu8; 300]];
        let mut store = BlobStore::open_encrypted(dir.path(), &key).expect("abrir cifrado");
        let mut hashes = Vec::new();
        for content in contents {
            hashes.push(store.put(content).expect("put"));
        }
        let dup = store.put(contents[0]).expect("put duplicado");
        assert_eq!(dup, hashes[0]);
        assert_eq!(store.ref_count(&hashes[0]), Some(2));
        for (hash, content) in hashes.iter().zip(contents.iter()) {
            assert_eq!(store.get(hash).expect("get"), *content);
            let raw = raw_blob_bytes(dir.path(), hash);
            assert_eq!(&raw[..4], &ENCRYPTED_MAGIC, "magic RCE1 en disco");
            assert_eq!(raw[4], ENCRYPTED_VERSION, "versión tras el magic");
            assert_ne!(raw.as_slice(), *content, "sin claro en disco");
            assert_eq!(raw.len(), ENVELOPE_OVERHEAD + content.len());
            let digest = Sha256::digest(*content);
            assert_eq!(
                &raw[ENVELOPE_HEADER..ENVELOPE_HEADER + ruscadb_crypto::NONCE_SIZE],
                &digest[..ruscadb_crypto::NONCE_SIZE],
                "nonce = sha256(contenido)[..24]"
            );
        }
        let first_raw = raw_blob_bytes(dir.path(), &hashes[0]);
        assert_eq!(
            raw_blob_bytes(dir.path(), &dup),
            first_raw,
            "dedup preservado"
        );
        let slice = store.get_range(&hashes[3], 10..20).expect("rango");
        assert_eq!(slice, contents[3][10..20].to_vec());
        drop(store);
        let mut reopened = BlobStore::open_encrypted(dir.path(), &key).expect("reabrir");
        for (hash, content) in hashes.iter().zip(contents.iter()) {
            assert_eq!(reopened.get(hash).expect("get tras reopen"), *content);
        }
        reopened.unref(&hashes[0]).expect("unref");
        assert!(reopened.contains(&hashes[0]), "sigue tras 1 unref");
        let clear = BlobStore::open(dir.path()).expect("abrir en claro");
        assert!(matches!(
            clear.get(&hashes[0]),
            Err(RuscaError::InvalidConfig(_))
        ));
    }

    /// AC-0013-04 — manipular un blob cifrado se detecta sin panics.
    ///
    /// Cubre flips en versión/nonce/ciphertext/tag, truncado del envelope,
    /// clave errónea y apertura sin clave: todo es error tipado.
    #[test] // @spec AC-0013-04
    fn test_ac_0013_04_tamper_is_detected() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let key = test_key();
        let mut store = BlobStore::open_encrypted(dir.path(), &key).expect("abrir cifrado");
        let hash = store.put(b"blob a proteger").expect("put");
        drop(store);
        let pristine = raw_blob_bytes(dir.path(), &hash);
        let tamper_cases: [usize; 4] = [0, 5, pristine.len() / 2, pristine.len() - 1];
        for offset in tamper_cases {
            let mut tampered = pristine.clone();
            tampered[offset] ^= 0x01;
            overwrite_raw(dir.path(), &hash, &tampered);
            let store = BlobStore::open_encrypted(dir.path(), &key).expect("reabrir");
            assert!(store.get(&hash).is_err(), "flip en {offset} detectado");
            assert!(
                store.get_range(&hash, 0..1).is_err(),
                "rango en {offset} detectado"
            );
        }
        overwrite_raw(dir.path(), &hash, &pristine[..10]);
        let store = BlobStore::open_encrypted(dir.path(), &key).expect("reabrir");
        assert!(matches!(
            store.get(&hash),
            Err(RuscaError::InvalidConfig(_))
        ));
        overwrite_raw(dir.path(), &hash, &pristine);
        let wrong = BlobStore::open_encrypted(dir.path(), &[0x77u8; 32]).expect("otra clave");
        assert!(wrong.get(&hash).is_err(), "clave errónea detectada");
        let clear = BlobStore::open(dir.path()).expect("abrir en claro");
        assert!(clear.get(&hash).is_err(), "sin clave detectado");
        let store = BlobStore::open_encrypted(dir.path(), &key).expect("reabrir");
        assert_eq!(store.get(&hash).expect("get"), b"blob a proteger");
    }

    /// `put` rechaza el fichero existente si es de otro modo o está truncado.
    #[test]
    fn test_encrypted_put_rejects_foreign_file() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let key = test_key();
        let content = b"contenido vigilado";
        let mut store = BlobStore::open_encrypted(dir.path(), &key).expect("abrir");
        let hash = store.put(content).expect("put");
        drop(store);
        overwrite_raw(dir.path(), &hash, b"claro ajeno");
        let mut store = BlobStore::open_encrypted(dir.path(), &key).expect("reabrir");
        assert!(matches!(
            store.put(content),
            Err(RuscaError::InvalidConfig(_))
        ));
        overwrite_raw(dir.path(), &hash, &[0x01u8; 10]);
        assert!(matches!(
            store.put(content),
            Err(RuscaError::InvalidConfig(_))
        ));
    }

    /// `Debug` indica el modo sin exponer jamás el material de clave.
    #[test]
    fn test_debug_redacts_key_material() {
        let dir = tempfile::tempdir().expect("dir temporal");
        let clear = BlobStore::open(dir.path()).expect("abrir");
        let shown = format!("{clear:?}");
        assert!(
            shown.contains("encrypted: false"),
            "debug en claro: {shown}"
        );
        let encrypted =
            BlobStore::open_encrypted(dir.path(), &[0xABu8; 32]).expect("abrir cifrado");
        let shown = format!("{encrypted:?}");
        assert!(shown.contains("encrypted: true"), "debug cifrado: {shown}");
        assert!(
            !shown.contains("171, 171"),
            "la clave no debe fugarse al log: {shown}"
        );
    }
    /// Frontera del envelope (45 B): magic, versión y longitud.
    ///
    /// El contenido claro que empieza por `0x01` (incluso largo) sigue siendo
    /// claro: solo el magic `RCE1` con longitud suficiente marca cifrado.
    #[test]
    fn test_envelope_boundary_classification() {
        assert_eq!(classify_bytes(&[]), StoredKind::Clear);
        assert_eq!(classify_bytes(&[0x02u8; 100]), StoredKind::Clear);
        assert_eq!(classify_bytes(&[0x01u8; 100]), StoredKind::Clear);
        let mut short = b"RCE1".to_vec();
        short.extend_from_slice(&[0x01u8; 10]);
        assert_eq!(classify_bytes(&short), StoredKind::Truncated);
        assert!(parse_blob_envelope(&short).is_err());
        let mut full = b"RCE1".to_vec();
        full.extend_from_slice(&[0x01u8; ENVELOPE_OVERHEAD - 4]);
        assert_eq!(classify_bytes(&full), StoredKind::Encrypted);
        let mut bad_version = full.clone();
        bad_version[4] = 0x02;
        assert!(parse_blob_envelope(&bad_version).is_err());
        let dir = tempfile::tempdir().expect("dir temporal");
        let probe = dir.path().join("probe.bin");
        fs::write(&probe, &short).expect("escribe");
        assert_eq!(
            stored_kind(&probe).expect("clasifica"),
            StoredKind::Truncated
        );
        fs::write(&probe, &full).expect("escribe");
        assert_eq!(
            stored_kind(&probe).expect("clasifica"),
            StoredKind::Encrypted
        );
        fs::write(&probe, vec![0x00u8; 100]).expect("escribe");
        assert_eq!(stored_kind(&probe).expect("clasifica"), StoredKind::Clear);
    }

    /// Sobrescribe el fichero del blob con `bytes` (simula manipulación en disco).
    fn overwrite_raw(root: &Path, hash: &BlobHash, bytes: &[u8]) {
        let first = hash.0.get(0..SHARD_WIDTH).unwrap_or("00");
        let second = hash.0.get(SHARD_WIDTH..SHARD_PREFIX_WIDTH).unwrap_or("00");
        let path = root
            .join(BLOBS_DIR)
            .join(first)
            .join(second)
            .join(format!("{hash}.{BLOB_EXTENSION}"));
        fs::write(path, bytes).expect("sobrescribe");
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
        ///
        /// El magic `RCE1` con longitud de envelope está reservado al modo
        /// cifrado y se excluye aquí (en modo claro sería ambiguo).
        #[test]
        fn prop_put_get_roundtrip(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            prop_assume!(classify_bytes(&bytes) == StoredKind::Clear);
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
            prop_assume!(classify_bytes(&bytes) == StoredKind::Clear);
            let start = start % bytes.len();
            let end = (start + len).min(bytes.len());
            let dir = tempfile::tempdir().expect("dir temporal");
            let mut store = BlobStore::open(dir.path()).expect("abrir store");
            let hash = store.put(&bytes).expect("put");
            let slice = store.get_range(&hash, start..end).expect("rango");
            prop_assert_eq!(slice, bytes[start..end].to_vec());
        }

        /// AC-0013-03 (propiedad): roundtrip cifrado para cualquier clave y contenido.
        #[test]
        fn prop_encrypted_blob_roundtrip(
            key in prop::array::uniform32(any::<u8>()),
            bytes in prop::collection::vec(any::<u8>(), 0..4096),
        ) {
            let dir = tempfile::tempdir().expect("dir temporal");
            let mut store = BlobStore::open_encrypted(dir.path(), &key).expect("abrir");
            let hash = store.put(&bytes).expect("put");
            let recovered = store.get(&hash).expect("get");
            prop_assert_eq!(&recovered, &bytes);
            let raw = raw_blob_bytes(dir.path(), &hash);
            prop_assert_eq!(&raw[..4], &ENCRYPTED_MAGIC);
            prop_assert_eq!(raw[4], ENCRYPTED_VERSION);
            prop_assert!(raw.as_slice() != bytes.as_slice(), "sin claro en disco");
            drop(store);
            let reopened = BlobStore::open_encrypted(dir.path(), &key).expect("reabrir");
            prop_assert_eq!(reopened.get(&hash).expect("get"), bytes);
        }

        /// AC-0013-03 (propiedad): `get_range` cifrado coincide con el slicing.
        #[test]
        fn prop_encrypted_get_range_matches_slice(
            key in prop::array::uniform32(any::<u8>()),
            bytes in prop::collection::vec(any::<u8>(), 1..1024),
            start in 0usize..1024,
            len in 0usize..1024,
        ) {
            let start = start % bytes.len();
            let end = (start + len).min(bytes.len());
            let dir = tempfile::tempdir().expect("dir temporal");
            let mut store = BlobStore::open_encrypted(dir.path(), &key).expect("abrir");
            let hash = store.put(&bytes).expect("put");
            let slice = store.get_range(&hash, start..end).expect("rango");
            prop_assert_eq!(slice, bytes[start..end].to_vec());
        }

        /// NF-0013-02 (propiedad): mismo contenido ⇒ mismo ciphertext (dedup).
        #[test]
        fn prop_encrypted_dedup_is_deterministic(
            key in prop::array::uniform32(any::<u8>()),
            bytes in prop::collection::vec(any::<u8>(), 0..1024),
        ) {
            let dir = tempfile::tempdir().expect("dir temporal");
            let mut store = BlobStore::open_encrypted(dir.path(), &key).expect("abrir");
            let first = store.put(&bytes).expect("put 1");
            let raw_first = raw_blob_bytes(dir.path(), &first);
            let second = store.put(&bytes).expect("put 2");
            prop_assert_eq!(&first, &second);
            prop_assert_eq!(raw_blob_bytes(dir.path(), &second), raw_first);
            prop_assert_eq!(store.ref_count(&first), Some(2));
        }
    }
}
