//! Keystore KMS-lite para tokens de sesión MCP (SPEC-0088).
//!
//! Un keystore local versionado gestiona keys simétricas de 256 bits:
//!
//! - **Emisión**: `issue_token` sella un payload fijo con
//!   XChaCha20-Poly1305 bajo la key activa y devuelve
//!   `v1.<kid>.<nonce>.<ciphertext>` (el token es auto-validable, sin
//!   estado en memoria del emisor).
//! - **Validación**: `validate` exige kid **activo**, tag AEAD correcto
//!   y payload exacto; la comparación del tag es constante por diseño
//!   (la hace el propio AEAD).
//! - **Rotación**: `rotate` retira la key activa (todos los tokens de
//!   esa key quedan invalidados) y crea la siguiente.
//! - **Persistencia**: con `load_or_create`, el fichero JSON versionado
//!   sobrevive a reinicios (permisos 0600 en unix) — KMS-lite sin KMS
//!   remoto: las keys viven en claro a propósito y el modelo de amenazas
//!   está documentado en `SECURITY.md`.
//!
//! Contrato SDD: SPEC-0088.

use crate::{KEY_SIZE, NONCE_SIZE, open, seal};
use ruscadb_core::RuscaError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Prefijo de versión de los tokens emitidos.
const TOKEN_VERSION: &str = "v1";

/// Contexto fijado dentro del payload sellado (evita reutilización
/// cross-protocol de un ciphertext del keystore en otro contexto).
const TOKEN_CONTEXT: &[u8] = b"ruscadb-mcp-session-v1";

/// Versión del formato del fichero keystore.
const FILE_FORMAT_VERSION: u32 = 1;

/// Estado de una key: apta para emitir y validar tokens.
const STATUS_ACTIVE: &str = "active";

/// Estado de una key: retirada por rotación; ya no valida tokens.
const STATUS_RETIRED: &str = "retired";

/// Número de segmentos de un token `v1.<kid>.<nonce>.<ciphertext>`.
const TOKEN_PARTS: usize = 4;

/// Registro de una key dentro del fichero keystore.
#[derive(Debug, Serialize, Deserialize)]
struct KeyRecord {
    /// Identificador público de la key (`k1`, `k2`, ...).
    kid: String,
    /// Material de la key en hexadecimal (256 bits, en claro: KMS-lite).
    secret: String,
    /// Ciclo de vida: `active` o `retired`.
    status: String,
}

/// Fichero keystore (JSON versionado).
#[derive(Debug, Serialize, Deserialize)]
struct KeystoreFile {
    /// Versión del formato del fichero.
    version: u32,
    /// Contador para el siguiente `kid`.
    next_id: u64,
    /// Keys conocidas (activas y retiradas).
    keys: Vec<KeyRecord>,
}

/// Keystore local versionado con rotación de keys (SPEC-0088).
#[derive(Debug)]
pub struct Keystore {
    /// Ruta del fichero; `None` = efímero (solo memoria).
    path: Option<PathBuf>,
    /// Estado del keystore serializado a disco.
    file: KeystoreFile,
}

impl Keystore {
    /// Crea un keystore efímero con una key activa nueva.
    ///
    /// Returns:
    ///     El keystore listo para emitir tokens (sin fichero asociado).
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: si el SO no proporciona entropía.
    pub fn ephemeral() -> Result<Self, RuscaError> {
        Ok(Self {
            path: None,
            file: Self::fresh_file()?,
        })
    }

    /// Abre el keystore en `path` o lo crea con una key activa.
    ///
    /// Args:
    ///     path: Ruta del fichero JSON del keystore.
    ///
    /// Returns:
    ///     El keystore cargado (o creado con permisos 0600 en unix).
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: fichero corrupto, versión
    ///         desconocida o sin key activa.
    ///     RuscaError::Io: error leyendo o escribiendo el fichero.
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<Self, RuscaError> {
        let path = path.as_ref();
        if path.exists() {
            let text = std::fs::read_to_string(path).map_err(|error| {
                RuscaError::InvalidConfig(format!(
                    "keystore ilegible en {}: {error}",
                    path.display()
                ))
            })?;
            let file: KeystoreFile = serde_json::from_str(&text).map_err(|error| {
                RuscaError::InvalidConfig(format!(
                    "keystore corrupto en {}: {error}",
                    path.display()
                ))
            })?;
            Self::check_file(&file, path)?;
            Ok(Self {
                path: Some(path.to_path_buf()),
                file,
            })
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(RuscaError::Io)?;
            }
            let keystore = Self {
                path: Some(path.to_path_buf()),
                file: Self::fresh_file()?,
            };
            keystore.persist()?;
            Ok(keystore)
        }
    }

    /// Emite un token de sesión bajo la key activa.
    ///
    /// Returns:
    ///     El token `v1.<kid>.<nonce>.<ciphertext>` en texto.
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: sin key activa, sin entropía o
    ///         sellado AEAD imposible.
    pub fn issue_token(&self) -> Result<String, RuscaError> {
        let key = self.active()?;
        let mut nonce = [0u8; NONCE_SIZE];
        getrandom::fill(&mut nonce).map_err(|error| {
            RuscaError::InvalidConfig(format!(
                "entropía del SO no disponible para emitir token de sesión: {error}"
            ))
        })?;
        let secret = decode_secret(key)?;
        let ciphertext = seal(&secret, &nonce, TOKEN_CONTEXT);
        if ciphertext.is_empty() {
            return Err(RuscaError::InvalidConfig(
                "el sellado AEAD del token devolvió vacío (imposible emitir sesión)".to_string(),
            ));
        }
        Ok(format!(
            "{TOKEN_VERSION}.{}.{}.{}",
            key.kid,
            hex::encode(nonce),
            hex::encode(ciphertext)
        ))
    }

    /// Valida un token de sesión contra las keys activas.
    ///
    /// Args:
    ///     token: Token `v1.<kid>.<nonce>.<ciphertext>` a validar.
    ///
    /// Returns:
    ///     `Ok(())` si el token es auténtico y su key sigue activa.
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: token mal formado, kid desconocido
    ///         o retirado, o tag AEAD inválido. El mensaje jamás incluye
    ///         el token ni el material de key.
    pub fn validate(&self, token: &str) -> Result<(), RuscaError> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != TOKEN_PARTS || parts[0] != TOKEN_VERSION {
            return Err(RuscaError::InvalidConfig(
                "token de sesión mal formado (formato esperado: v1.kid.nonce.ciphertext)"
                    .to_string(),
            ));
        }
        let kid = parts[1];
        let record = self
            .file
            .keys
            .iter()
            .find(|key| key.kid == kid)
            .ok_or_else(|| {
                RuscaError::InvalidConfig(
                    "kid desconocido en el token de sesión (posible keystore reiniciado)"
                        .to_string(),
                )
            })?;
        if record.status != STATUS_ACTIVE {
            return Err(RuscaError::InvalidConfig(format!(
                "key {kid} retirada por rotación: la sesión dejó de ser válida"
            )));
        }
        let nonce = hex::decode(parts[2]).map_err(|_| {
            RuscaError::InvalidConfig("nonce del token no es hexadecimal válido".to_string())
        })?;
        let nonce: [u8; NONCE_SIZE] = nonce.try_into().map_err(|raw: Vec<u8>| {
            RuscaError::InvalidConfig(format!(
                "nonce del token mide {} bytes; se esperaban {NONCE_SIZE}",
                raw.len()
            ))
        })?;
        let ciphertext = hex::decode(parts[3]).map_err(|_| {
            RuscaError::InvalidConfig("ciphertext del token no es hexadecimal válido".to_string())
        })?;
        let secret = decode_secret(record)?;
        let payload = open(&secret, &nonce, &ciphertext)?;
        if payload != TOKEN_CONTEXT {
            return Err(RuscaError::InvalidConfig(
                "payload de sesión con contexto inesperado (token rechazado)".to_string(),
            ));
        }
        Ok(())
    }

    /// Rota el keystore: retira la key activa y crea la siguiente.
    ///
    /// Returns:
    ///     `Ok(())` tras emitir la nueva key (y persistirla si aplica).
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: sin entropía para la nueva key.
    ///     RuscaError::Io: fallo al persistir el fichero.
    pub fn rotate(&mut self) -> Result<(), RuscaError> {
        for key in &mut self.file.keys {
            if key.status == STATUS_ACTIVE {
                key.status = STATUS_RETIRED.to_string();
            }
        }
        let next = self.file.next_id;
        self.file.keys.push(Self::generate_key(next)?);
        self.file.next_id += 1;
        self.persist()
    }

    /// Identificador de la key activa actual.
    ///
    /// Returns:
    ///     El `kid` activo (p. ej. `k1`).
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: si no hay key activa.
    pub fn active_kid(&self) -> Result<&str, RuscaError> {
        Ok(&self.active()?.kid)
    }

    /// Ruta del fichero asociado (`None` si es efímero).
    ///
    /// Returns:
    ///     La ruta persistida del keystore.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Construye un estado inicial con `k1` activa y `next_id = 2`.
    ///
    /// Returns:
    ///     El fichero keystore recién generado en memoria.
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: si el SO no proporciona entropía.
    fn fresh_file() -> Result<KeystoreFile, RuscaError> {
        Ok(KeystoreFile {
            version: FILE_FORMAT_VERSION,
            next_id: 2,
            keys: vec![Self::generate_key(1)?],
        })
    }

    /// Genera un registro de key con `kid` derivado de `id`.
    ///
    /// Args:
    ///     id: Entero identificador de la key (base del `kid`).
    ///
    /// Returns:
    ///     El registro activo con 32 bytes de entropía del SO.
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: si el SO no proporciona entropía.
    fn generate_key(id: u64) -> Result<KeyRecord, RuscaError> {
        let mut secret = [0u8; KEY_SIZE];
        getrandom::fill(&mut secret).map_err(|error| {
            RuscaError::InvalidConfig(format!(
                "entropía del SO no disponible para generar la key {id}: {error}"
            ))
        })?;
        Ok(KeyRecord {
            kid: format!("k{id}"),
            secret: hex::encode(secret),
            status: STATUS_ACTIVE.to_string(),
        })
    }

    /// Devuelve la key activa del keystore.
    ///
    /// Returns:
    ///     El registro con estado `active`.
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: si ninguna key está activa.
    fn active(&self) -> Result<&KeyRecord, RuscaError> {
        self.file
            .keys
            .iter()
            .find(|key| key.status == STATUS_ACTIVE)
            .ok_or_else(|| {
                RuscaError::InvalidConfig(
                    "keystore sin key activa: emite un token o rota el keystore".to_string(),
                )
            })
    }

    /// Comprueba versión y presencia de key activa de un fichero leído.
    ///
    /// Args:
    ///     file: Contenido parseado del fichero keystore.
    ///     path: Ruta del fichero (para mensajes accionables).
    ///
    /// Returns:
    ///     `Ok(())` si el formato es utilizable.
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: versión desconocida o sin key activa.
    fn check_file(file: &KeystoreFile, path: &Path) -> Result<(), RuscaError> {
        if file.version != FILE_FORMAT_VERSION {
            return Err(RuscaError::InvalidConfig(format!(
                "keystore en {}: versión {FILE_FORMAT_VERSION} esperada, encontrada {}",
                path.display(),
                file.version
            )));
        }
        if !file.keys.iter().any(|key| key.status == STATUS_ACTIVE) {
            return Err(RuscaError::InvalidConfig(format!(
                "keystore en {} sin key activa (¿fichero manipulado?)",
                path.display()
            )));
        }
        // Input externo (VAL): un fichero con kids duplicados rompería la
        // búsqueda de `validate` (primer `kid` coincidente gana).
        let mut seen: Vec<&str> = Vec::new();
        for key in &file.keys {
            if seen.contains(&key.kid.as_str()) {
                return Err(RuscaError::InvalidConfig(format!(
                    "keystore en {}: kid duplicado '{}' (fichero manipulado)",
                    path.display(),
                    key.kid
                )));
            }
            seen.push(&key.kid);
        }
        Ok(())
    }

    /// Escribe el fichero de forma atómica (tmp + rename, 0600 en unix).
    ///
    /// Returns:
    ///     `Ok(())` si el estado quedó persistido (o no hay fichero).
    ///
    /// Raises:
    ///     RuscaError::InvalidConfig: fallo al serializar el JSON.
    ///     RuscaError::Io: fallo de escritura, permisos o rename.
    fn persist(&self) -> Result<(), RuscaError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::to_string_pretty(&self.file).map_err(|error| {
            RuscaError::InvalidConfig(format!("keystore no serializable: {error}"))
        })?;
        let tmp = path.with_extension("keystore.tmp");
        std::fs::write(&tmp, json).map_err(RuscaError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp, permissions).map_err(RuscaError::Io)?;
        }
        std::fs::rename(&tmp, path).map_err(RuscaError::Io)?;
        Ok(())
    }
}

/// Decodifica el `secret` hex de un registro a `[u8; KEY_SIZE]`.
///
/// Args:
///     record: Registro de key del keystore.
///
/// Returns:
///     La key simétrica de 256 bits.
///
/// Raises:
///     RuscaError::InvalidConfig: hex inválido o longitud incorrecta.
fn decode_secret(record: &KeyRecord) -> Result<[u8; KEY_SIZE], RuscaError> {
    let raw = hex::decode(&record.secret).map_err(|_| {
        RuscaError::InvalidConfig(format!(
            "secret de la key {} no es hexadecimal válido",
            record.kid
        ))
    })?;
    let secret: [u8; KEY_SIZE] = raw.try_into().map_err(|raw: Vec<u8>| {
        RuscaError::InvalidConfig(format!(
            "secret de la key {} mide {} bytes; se esperaban {KEY_SIZE}",
            record.kid,
            raw.len()
        ))
    })?;
    Ok(secret)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Emisión y validación round-trip del token.
    #[test]
    fn issue_and_validate_roundtrip() {
        let keystore = Keystore::ephemeral().expect("keystore efímero");
        let token = keystore.issue_token().expect("token emitido");
        assert!(keystore.validate(&token).is_ok(), "token recién emitido");
        assert!(token.starts_with("v1.k1."), "formato v1.kid");
    }

    /// La rotación retira la key activa y el nuevo token valida.
    #[test]
    fn rotate_retires_old_key_and_issues_new_one() {
        let mut keystore = Keystore::ephemeral().expect("keystore efímero");
        let old = keystore.issue_token().expect("token antiguo");
        keystore.rotate().expect("rotación");
        assert!(
            keystore.validate(&old).is_err(),
            "el token de la key retirada deja de validar"
        );
        let new = keystore.issue_token().expect("token nuevo");
        assert!(keystore.validate(&new).is_ok(), "el kid nuevo valida");
        assert_eq!(keystore.active_kid().expect("kid activo"), "k2");
    }

    /// Tokens corruptos o de kid desconocido se rechazan sin fugas.
    #[test]
    fn tampered_and_unknown_kid_tokens_are_rejected() {
        let keystore = Keystore::ephemeral().expect("keystore efímero");
        let token = keystore.issue_token().expect("token emitido");
        let mut corrupted = token.clone();
        corrupted.push('A');
        let error = keystore.validate(&corrupted).expect_err("debe rechazar");
        assert!(
            !error.to_string().contains(&token),
            "el error no puede filtrar el token: {error}"
        );
        let unknown = "v1.k99.aa.bb";
        assert!(keystore.validate(unknown).is_err(), "kid desconocido");
    }

    /// El keystore persistido recarga y valida tokens en otro proceso.
    #[test]
    fn persisted_keystore_survives_reload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.keystore");
        let mut keystore = Keystore::load_or_create(&path).expect("creación");
        let token = keystore.issue_token().expect("token emitido");
        keystore.rotate().expect("rotación persistida");
        drop(keystore);

        let reloaded = Keystore::load_or_create(&path).expect("recarga");
        assert_eq!(
            reloaded.active_kid().expect("kid"),
            "k2",
            "rotate persistió"
        );
        assert!(
            reloaded.validate(&token).is_err(),
            "la key retirada sigue retirada tras recargar"
        );
        let text = std::fs::read_to_string(&path).expect("lectura");
        assert!(text.contains("\"version\": 1"), "formato versionado");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "permisos 0600 en unix");
        }
    }
}
