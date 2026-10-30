//! KMS local (SPEC-0143): envoltura de claves con AEAD real.
//!
//! Sustituye el XOR placeholder por **XChaCha20-Poly1305** de `ruscadb-crypto`
//! (cifrado autenticado, nonce de 192 bits). El nonce se compone de contador
//! monotono + reloj + mezcla `splitmix64`, de modo que nunca se repite con la
//! misma clave.
//!
//! No es un KMS/HSM remoto: la clave vive en memoria. El contrato permite
//! inyectar un proveedor externo cuando exista (credenciales/decision
//! operativa), pero el cifrado ya es criptograficamente solido.
//!
//! Referencias: RFC 8439 (ChaCha20-Poly1305); libsodium `crypto_aead_xchacha20poly1305`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ruscadb_core::RuscaError;
use ruscadb_crypto::{KEY_SIZE, NONCE_SIZE, open, seal};

/// Proveedor KMS local con AEAD.
#[derive(Debug)]
pub struct KmsProvider {
    key: [u8; KEY_SIZE],
    counter: AtomicU64,
}

/// `splitmix64` para mezclar contador/reloj y expandir claves.
fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Expande bytes arbitrarios a una clave de 256 bits.
///
/// No es un KDF de contraseñas: para produccion se debe usar
/// [`KmsProvider::from_key`] con 32 bytes aleatorios o `derive_key` (Argon2id).
fn expand_key(input: &[u8]) -> [u8; KEY_SIZE] {
    let mut key = [0u8; KEY_SIZE];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    if input.is_empty() {
        state ^= 0xDEAD_BEEF_CAFE_F00Du64;
    }
    for (index, byte) in input.iter().enumerate() {
        state ^= u64::from(*byte).wrapping_mul(0x0000_0100_0000_01B3);
        state = splitmix64(state);
        key[index % KEY_SIZE] ^= (state >> ((index % 8) * 8)) as u8;
    }
    if input.is_empty() {
        for (index, slot) in key.iter_mut().enumerate() {
            state = splitmix64(state.wrapping_add(index as u64));
            *slot = (state >> 32) as u8;
        }
    }
    key
}

impl KmsProvider {
    /// Crea el proveedor a partir de bytes (se expanden a 32 bytes).
    ///
    /// Args:
    ///     key: Material de clave (idealmente 32 bytes aleatorios).
    ///
    /// Returns:
    ///     Proveedor listo.
    pub fn new(key: Vec<u8>) -> Self {
        Self::from_key(expand_key(&key))
    }

    /// Crea el proveedor con una clave de 256 bits.
    ///
    /// Args:
    ///     key: Clave de 32 bytes.
    ///
    /// Returns:
    ///     Proveedor listo.
    pub fn from_key(key: [u8; KEY_SIZE]) -> Self {
        Self {
            key,
            counter: AtomicU64::new(0),
        }
    }

    /// Cifra y autentica un payload (nonce prepended).
    ///
    /// Args:
    ///     plaintext: Bytes originales.
    ///
    /// Returns:
    ///     `nonce (24 B) || ciphertext`.
    pub fn wrap(&self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = self.next_nonce();
        let ciphertext = seal(&self.key, &nonce, plaintext);
        let mut out = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        out
    }

    /// Descifra y verifica un payload; devuelve vacio si fue manipulado.
    ///
    /// Args:
    ///     ciphertext: Salida de [`Self::wrap`].
    ///
    /// Returns:
    ///     Plaintext, o vacio si el AEAD no autentica.
    pub fn unwrap(&self, ciphertext: &[u8]) -> Vec<u8> {
        self.try_unwrap(ciphertext).unwrap_or_default()
    }

    /// Descifra con error accionable (para propagar tamper).
    ///
    /// Args:
    ///     ciphertext: Salida de [`Self::wrap`].
    ///
    /// Returns:
    ///     Plaintext autenticado.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el ciphertext es corto o el AEAD
    ///     no autentica.
    pub fn try_unwrap(&self, ciphertext: &[u8]) -> Result<Vec<u8>, RuscaError> {
        if ciphertext.len() < NONCE_SIZE {
            return Err(RuscaError::InvalidConfig(
                "KMS: ciphertext mas corto que el nonce".to_string(),
            ));
        }
        let (nonce_bytes, payload) = ciphertext.split_at(NONCE_SIZE);
        let mut nonce = [0u8; NONCE_SIZE];
        nonce.copy_from_slice(nonce_bytes);
        open(&self.key, &nonce, payload)
    }

    /// Genera un nonce unico (contador + reloj + mezcla).
    fn next_nonce(&self) -> [u8; NONCE_SIZE] {
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0);
        let mut nonce = [0u8; NONCE_SIZE];
        nonce[0..8].copy_from_slice(&counter.to_le_bytes());
        nonce[8..16].copy_from_slice(&nanos.to_le_bytes());
        nonce[16..24].copy_from_slice(&splitmix64(counter ^ nanos).to_le_bytes());
        nonce
    }
}

#[cfg(test)]
mod tests {
    use super::KmsProvider;

    #[test] // @spec AC-0143-01
    fn test_ac_0143_01_kms_roundtrip() {
        let provider = KmsProvider::new(vec![0xAB, 0xCD]);
        let plaintext = b"secret key";
        let wrapped = provider.wrap(plaintext);
        assert_ne!(wrapped, plaintext);
        assert!(wrapped.len() > plaintext.len(), "incluye nonce + tag");
        assert_eq!(provider.unwrap(&wrapped), plaintext);
        assert_eq!(provider.try_unwrap(&wrapped).expect("aead"), plaintext);
    }

    /// Nonces distintos y tamper detectado (AEAD).
    #[test]
    fn nonces_are_unique_and_tamper_is_rejected() {
        let provider = KmsProvider::new(vec![1, 2, 3, 4]);
        let first = provider.wrap(b"hola");
        let second = provider.wrap(b"hola");
        assert_ne!(first[..24], second[..24], "nonces distintos");

        let mut tampered = first.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        assert!(provider.try_unwrap(&tampered).is_err());
        assert!(provider.unwrap(&tampered).is_empty());

        // Clave distinta no descifra.
        let other = KmsProvider::new(vec![9, 9, 9, 9]);
        assert!(other.try_unwrap(&first).is_err());
        assert!(provider.try_unwrap(&[]).is_err());
    }
}
