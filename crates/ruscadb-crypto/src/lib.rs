//! # ruscadb-crypto
//!
//! Cifrado en reposo de RuscaDB: AEAD **XChaCha20-Poly1305**
//! (confidencialidad e integridad) y derivación de clave con **Argon2id**
//! (resistencia a fuerza bruta sobre passphrases).
//! Especificación: `specs/crypto.md` (SPEC-0011).
//!
//! ## Propiedades de seguridad
//!
//! - **AEAD**: cualquier manipulación del ciphertext o del tag Poly1305 hace
//!   fallar [`open`] con [`RuscaError::InvalidConfig`]; nunca devuelve texto
//!   plano corrupto.
//! - **Nonce de 192 bits**: el espacio de nonces de XChaCha20 es lo bastante
//!   grande para generarlos aleatoriamente sin coordinación (no requiere
//!   contador).
//! - **KDF salada**: Argon2id deriva una clave de 256 bits de una passphrase y
//!   un salt; el mismo par produce siempre la misma clave (determinismo).
//! - **Zeroización**: el material de clave temporal se envuelve en
//!   [`zeroize::Zeroizing`] y se limpia al salir de ámbito.
//!
//! ## Modelo de amenazas (STRIDE) cubierto
//!
//! - **Spoofing / Tampering**: el tag Poly1305 autentica origen e integridad.
//! - **Information Disclosure (en reposo)**: los datos se almacenan cifrados.
//! - **Elevation of privilege vía KDF débil**: Argon2id con parámetros por
//!   defecto encarece el ataque de diccionario.
//! - **Repudiation**: no aplica (sin firma de autoría en esta capa).

#![forbid(unsafe_code)]

use argon2::Argon2;
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use ruscadb_core::RuscaError;
use zeroize::Zeroizing;

/// Tamaño de la clave simétrica en bytes (256 bits).
pub const KEY_SIZE: usize = 32;

/// Tamaño del nonce extendido en bytes (192 bits, XChaCha20).
pub const NONCE_SIZE: usize = 24;

/// Mensaje de error ante un ciphertext no autenticable.
const INVALID_CIPHERTEXT: &str = "ciphertext no auténtico o malformado (AEAD XChaCha20-Poly1305)";

/// Sella `plaintext` con AEAD XChaCha20-Poly1305.
///
/// Args:
///     key: clave simétrica de [`KEY_SIZE`] bytes.
///     nonce: nonce de [`NONCE_SIZE`] bytes; NUNCA debe repetirse bajo la
///         misma clave.
///     plaintext: mensaje en claro a cifrar.
///
/// Returns:
///     El ciphertext con el tag Poly1305 (16 bytes) anexado al final. Si el
///     AEAD rechazase el mensaje (solo posible con entradas mayores de
///     ~256 GiB) devuelve un vector vacío, que [`open`] no podrá abrir.
pub fn seal(key: &[u8; KEY_SIZE], nonce: &[u8; NONCE_SIZE], plaintext: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(&Key::from(*key));
    cipher
        .encrypt(&XNonce::from(*nonce), plaintext)
        .unwrap_or_default()
}

/// Abre un ciphertext sellado con [`seal`], verificando el tag AEAD.
///
/// Args:
///     key: clave simétrica de [`KEY_SIZE`] bytes.
///     nonce: nonce de [`NONCE_SIZE`] bytes usado en el sellado.
///     ciphertext: bytes producidos por [`seal`].
///
/// Returns:
///     El mensaje en claro original.
///
/// Raises:
///     RuscaError::InvalidConfig: si el ciphertext o el tag AEAD no son
///         válidos (manipulación, clave/nonce incorrectos o formato inválido).
pub fn open(
    key: &[u8; KEY_SIZE],
    nonce: &[u8; NONCE_SIZE],
    ciphertext: &[u8],
) -> Result<Vec<u8>, RuscaError> {
    let cipher = XChaCha20Poly1305::new(&Key::from(*key));
    cipher
        .decrypt(&XNonce::from(*nonce), ciphertext)
        .map_err(|_| RuscaError::InvalidConfig(INVALID_CIPHERTEXT.to_string()))
}

/// Deriva una clave de 256 bits a partir de `passphrase` y `salt` con Argon2id.
///
/// Args:
///     passphrase: secreto elegido por el usuario.
///     salt: sal única por clave (se recomienda >= 8 bytes).
///
/// Returns:
///     La clave derivada de [`KEY_SIZE`] bytes.
///
/// Raises:
///     RuscaError::InvalidConfig: si Argon2id rechaza la entrada (p. ej. salt
///         demasiado corta/larga o fallo de memoria).
pub fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; KEY_SIZE], RuscaError> {
    let mut key = Zeroizing::new([0u8; KEY_SIZE]);
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut *key)
        .map_err(|error| {
            RuscaError::InvalidConfig(format!("derivación de clave Argon2id falló: {error}"))
        })?;
    Ok(*key)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// AC-0011-01 — sellar y abrir recupera el mensaje idéntico.
    #[test] // @spec AC-0011-01
    fn test_ac_0011_01_seal_open_roundtrip() {
        let key = [0x42u8; KEY_SIZE];
        let nonce = [0x24u8; NONCE_SIZE];
        let plaintext = b"mensaje secreto de RuscaDB";

        let ciphertext = seal(&key, &nonce, plaintext);

        // El tag Poly1305 añade 16 bytes y el ciphertext no expone el claro.
        assert_eq!(ciphertext.len(), plaintext.len() + 16);
        assert_ne!(ciphertext.as_slice(), plaintext.as_slice());

        let recovered = open(&key, &nonce, &ciphertext).expect("open");
        assert_eq!(recovered, plaintext);
    }

    /// AC-0011-02 — alterar un byte del ciphertext o del tag se detecta.
    #[test] // @spec AC-0011-02
    fn test_ac_0011_02_tamper_is_detected() {
        let key = [0x11u8; KEY_SIZE];
        let nonce = [0x22u8; NONCE_SIZE];
        let ciphertext = seal(&key, &nonce, b"contenido a proteger");

        // Flip en el cuerpo del ciphertext (byte 0).
        let mut tampered_body = ciphertext.clone();
        tampered_body[0] ^= 0x01;
        assert!(open(&key, &nonce, &tampered_body).is_err());

        // Flip en el tag de autenticación (último byte).
        let mut tampered_tag = ciphertext.clone();
        let last = tampered_tag.len() - 1;
        tampered_tag[last] ^= 0x01;
        assert!(open(&key, &nonce, &tampered_tag).is_err());
    }

    /// AC-0011-03 — una clave o un nonce distintos fallan la verificación.
    #[test] // @spec AC-0011-03
    fn test_ac_0011_03_wrong_key_or_nonce_fails() {
        let key = [0x01u8; KEY_SIZE];
        let nonce = [0x02u8; NONCE_SIZE];
        let ciphertext = seal(&key, &nonce, b"dato autenticado");

        let other_key = [0x03u8; KEY_SIZE];
        let other_nonce = [0x04u8; NONCE_SIZE];

        assert!(open(&other_key, &nonce, &ciphertext).is_err());
        assert!(open(&key, &other_nonce, &ciphertext).is_err());

        // Con la clave y el nonce correctos sigue abriendo.
        assert_eq!(
            open(&key, &nonce, &ciphertext).expect("open"),
            b"dato autenticado"
        );
    }

    /// AC-0011-04 — `derive_key` es determinista y sensible a sus entradas.
    #[test] // @spec AC-0011-04
    fn test_ac_0011_04_derive_key_is_deterministic() {
        let salt = b"sal-de-16-bytes!";
        let first = derive_key("passphrase", salt).expect("derive");
        let second = derive_key("passphrase", salt).expect("derive");
        assert_eq!(first, second, "el mismo par debe dar la misma clave");

        let other_salt = derive_key("passphrase", b"otra-sal-16-bytes").expect("derive");
        assert_ne!(first, other_salt, "otra sal debe dar otra clave");

        let other_pass = derive_key("otra-passphrase", salt).expect("derive");
        assert_ne!(first, other_pass, "otra passphrase debe dar otra clave");
    }

    proptest! {
        /// Propiedad: para cualquier clave, nonce y mensaje, `open(seal(x)) == x`.
        #[test]
        fn prop_seal_open_roundtrip(
            key in prop::array::uniform32(any::<u8>()),
            nonce in prop::array::uniform24(any::<u8>()),
            message in prop::collection::vec(any::<u8>(), 0..4096),
        ) {
            let ciphertext = seal(&key, &nonce, &message);
            let recovered = open(&key, &nonce, &ciphertext).expect("open");
            prop_assert_eq!(recovered, message);
        }
    }
}
