//! ML-KEM (FIPS 203) e híbrido X25519 + ML-KEM (SPEC-0180).
//!
//! La implementación usa `libcrux-ml-kem` (formalmente verificada con hax/F*)
//! para ML-KEM-768 y `x25519-dalek` para el componente ECDH. El secreto híbrido
//! es `SHA-256(ml_kem_ss || x25519_ss)` (32 bytes).
//!
//! **Estado de seguridad**: la implementación es formalmente verificada y se
//! contrasta con vectores ACVP, pero **no** ha pasado una auditoría externa
//! independiente. La feature `post-quantum` está OFF por defecto.

#![allow(clippy::too_many_arguments)]

use libcrux_ml_kem::mlkem768::{self, MlKem768Ciphertext, MlKem768PrivateKey, MlKem768PublicKey};
use libcrux_ml_kem::{KEY_GENERATION_SEED_SIZE, SHARED_SECRET_SIZE};
use ruscadb_core::RuscaError;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

/// Tamaño de la clave pública ML-KEM-768.
pub const ML_KEM_768_PUBLIC_KEY_BYTES: usize = 1184;
/// Tamaño de la clave privada ML-KEM-768.
pub const ML_KEM_768_PRIVATE_KEY_BYTES: usize = 2400;
/// Tamaño del ciphertext ML-KEM-768.
pub const ML_KEM_768_CIPHERTEXT_BYTES: usize = 1088;
/// Tamaño del secreto compartido.
pub const SHARED_SECRET_BYTES: usize = 32;
/// Tamaño de la clave pública híbrida (`ml_kem_pk || x25519_pk`).
pub const HYBRID_PUBLIC_KEY_BYTES: usize = ML_KEM_768_PUBLIC_KEY_BYTES + 32;
/// Tamaño de la clave secreta híbrida (`ml_kem_pk || ml_kem_sk || x25519_sk`).
pub const HYBRID_SECRET_KEY_BYTES: usize =
    ML_KEM_768_PUBLIC_KEY_BYTES + ML_KEM_768_PRIVATE_KEY_BYTES + 32;
/// Tamaño del ciphertext híbrido (`ml_kem_ct || x25519_ephemeral_pk`).
pub const HYBRID_CIPHERTEXT_BYTES: usize = ML_KEM_768_CIPHERTEXT_BYTES + 32;

/// Par de claves ML-KEM-768 serializable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MlKemKeyPair {
    public_key: Vec<u8>,
    private_key: Vec<u8>,
}

impl MlKemKeyPair {
    /// Clave pública (1184 bytes).
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Clave privada (2400 bytes).
    pub fn private_key(&self) -> &[u8] {
        &self.private_key
    }

    /// Serializa `public_key || private_key`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes =
            Vec::with_capacity(ML_KEM_768_PUBLIC_KEY_BYTES + ML_KEM_768_PRIVATE_KEY_BYTES);
        bytes.extend_from_slice(&self.public_key);
        bytes.extend_from_slice(&self.private_key);
        bytes
    }

    /// Reconstruye un par a partir de `public_key || private_key`.
    ///
    /// Args:
    ///     bytes: Serialización de [`MlKemKeyPair::to_bytes`].
    ///
    /// Returns:
    ///     El par validado.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el tamaño o las claves no son
    ///     válidos.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RuscaError> {
        let expected = ML_KEM_768_PUBLIC_KEY_BYTES + ML_KEM_768_PRIVATE_KEY_BYTES;
        if bytes.len() != expected {
            return Err(RuscaError::InvalidConfig(format!(
                "clave ML-KEM serializada de {} bytes; se esperaban {expected}",
                bytes.len()
            )));
        }
        let (public_key, private_key) = bytes.split_at(ML_KEM_768_PUBLIC_KEY_BYTES);
        Self::from_parts(public_key, private_key)
    }

    /// Construye y valida un par a partir de sus partes.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si las longitudes o la validación
    ///     FIPS 203 fallan.
    pub fn from_parts(public_key: &[u8], private_key: &[u8]) -> Result<Self, RuscaError> {
        if public_key.len() != ML_KEM_768_PUBLIC_KEY_BYTES {
            return Err(RuscaError::InvalidConfig(format!(
                "clave pública ML-KEM de {} bytes; se esperaban {ML_KEM_768_PUBLIC_KEY_BYTES}",
                public_key.len()
            )));
        }
        if private_key.len() != ML_KEM_768_PRIVATE_KEY_BYTES {
            return Err(RuscaError::InvalidConfig(format!(
                "clave privada ML-KEM de {} bytes; se esperaban {ML_KEM_768_PRIVATE_KEY_BYTES}",
                private_key.len()
            )));
        }
        let public = MlKem768PublicKey::try_from(public_key).map_err(|_| {
            RuscaError::InvalidConfig("clave pública ML-KEM mal formada".to_string())
        })?;
        if !mlkem768::validate_public_key(&public) {
            return Err(RuscaError::InvalidConfig(
                "clave pública ML-KEM inválida (FIPS 203 §7.2)".to_string(),
            ));
        }
        let private = MlKem768PrivateKey::try_from(private_key).map_err(|_| {
            RuscaError::InvalidConfig("clave privada ML-KEM mal formada".to_string())
        })?;
        let dummy_ciphertext = MlKem768Ciphertext::default();
        if !mlkem768::validate_private_key(&private, &dummy_ciphertext) {
            return Err(RuscaError::InvalidConfig(
                "clave privada ML-KEM inválida (FIPS 203 §7.3)".to_string(),
            ));
        }
        Ok(Self {
            public_key: public_key.to_vec(),
            private_key: private_key.to_vec(),
        })
    }
}

/// Operaciones ML-KEM-768.
#[derive(Debug, Clone, Copy, Default)]
pub struct MlKem;

impl MlKem {
    /// Genera un par de claves con entropía del SO.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el SO no proporciona entropía.
    pub fn keygen() -> Result<MlKemKeyPair, RuscaError> {
        let mut seed = [0u8; KEY_GENERATION_SEED_SIZE];
        getrandom::fill(&mut seed).map_err(|error| {
            RuscaError::InvalidConfig(format!(
                "entropía del SO no disponible para ML-KEM keygen: {error}"
            ))
        })?;
        Ok(Self::keygen_from_seed(seed))
    }

    /// Genera un par de claves de forma determinista (KAT).
    pub fn keygen_from_seed(seed: [u8; KEY_GENERATION_SEED_SIZE]) -> MlKemKeyPair {
        let key_pair = mlkem768::generate_key_pair(seed);
        MlKemKeyPair {
            public_key: key_pair.public_key().as_slice().to_vec(),
            private_key: key_pair.private_key().as_slice().to_vec(),
        }
    }

    /// Encapsula un secreto hacia una clave pública.
    ///
    /// Returns:
    ///     `(ciphertext, shared_secret)`.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la clave pública no es válida o
    ///     falta entropía.
    pub fn encapsulate(
        public_key: &[u8],
    ) -> Result<(Vec<u8>, [u8; SHARED_SECRET_BYTES]), RuscaError> {
        let mut randomness = [0u8; SHARED_SECRET_SIZE];
        getrandom::fill(&mut randomness).map_err(|error| {
            RuscaError::InvalidConfig(format!(
                "entropía del SO no disponible para ML-KEM encapsulate: {error}"
            ))
        })?;
        Self::encapsulate_deterministic(public_key, randomness)
    }

    /// Encapsula con randomness explícita (KAT).
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la clave pública no es válida.
    pub fn encapsulate_deterministic(
        public_key: &[u8],
        randomness: [u8; SHARED_SECRET_SIZE],
    ) -> Result<(Vec<u8>, [u8; SHARED_SECRET_BYTES]), RuscaError> {
        if public_key.len() != ML_KEM_768_PUBLIC_KEY_BYTES {
            return Err(RuscaError::InvalidConfig(format!(
                "clave pública ML-KEM de {} bytes; se esperaban {ML_KEM_768_PUBLIC_KEY_BYTES}",
                public_key.len()
            )));
        }
        let public = MlKem768PublicKey::try_from(public_key).map_err(|_| {
            RuscaError::InvalidConfig("clave pública ML-KEM mal formada".to_string())
        })?;
        if !mlkem768::validate_public_key(&public) {
            return Err(RuscaError::InvalidConfig(
                "clave pública ML-KEM inválida (FIPS 203 §7.2)".to_string(),
            ));
        }
        let (ciphertext, shared) = mlkem768::encapsulate(&public, randomness);
        Ok((ciphertext.as_slice().to_vec(), shared))
    }

    /// Desencapsula el secreto de un ciphertext.
    ///
    /// FIPS 203 usa *implicit rejection*: un ciphertext manipulado produce un
    /// secreto distinto (no un error). El test AC-0180-02 verifica esa
    /// propiedad.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la clave privada o el ciphertext
    ///     están mal formados o la clave privada es inválida.
    pub fn decapsulate(
        private_key: &[u8],
        ciphertext: &[u8],
    ) -> Result<[u8; SHARED_SECRET_BYTES], RuscaError> {
        if private_key.len() != ML_KEM_768_PRIVATE_KEY_BYTES {
            return Err(RuscaError::InvalidConfig(format!(
                "clave privada ML-KEM de {} bytes; se esperaban {ML_KEM_768_PRIVATE_KEY_BYTES}",
                private_key.len()
            )));
        }
        if ciphertext.len() != ML_KEM_768_CIPHERTEXT_BYTES {
            return Err(RuscaError::InvalidConfig(format!(
                "ciphertext ML-KEM de {} bytes; se esperaban {ML_KEM_768_CIPHERTEXT_BYTES}",
                ciphertext.len()
            )));
        }
        let private = MlKem768PrivateKey::try_from(private_key).map_err(|_| {
            RuscaError::InvalidConfig("clave privada ML-KEM mal formada".to_string())
        })?;
        let ciphertext = MlKem768Ciphertext::try_from(ciphertext)
            .map_err(|_| RuscaError::InvalidConfig("ciphertext ML-KEM mal formado".to_string()))?;
        if !mlkem768::validate_private_key(&private, &ciphertext) {
            return Err(RuscaError::InvalidConfig(
                "clave privada ML-KEM inválida (FIPS 203 §7.3)".to_string(),
            ));
        }
        Ok(mlkem768::decapsulate(&private, &ciphertext))
    }
}

/// Par de claves híbrido X25519 + ML-KEM-768.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HybridKeyPair {
    ml_kem: MlKemKeyPair,
    x25519_secret: [u8; 32],
}

impl HybridKeyPair {
    /// Clave pública híbrida (`ml_kem_pk || x25519_pk`).
    pub fn public_key(&self) -> Vec<u8> {
        let x25519_public = X25519PublicKey::from(&StaticSecret::from(self.x25519_secret));
        let mut bytes = Vec::with_capacity(HYBRID_PUBLIC_KEY_BYTES);
        bytes.extend_from_slice(self.ml_kem.public_key());
        bytes.extend_from_slice(x25519_public.as_bytes());
        bytes
    }

    /// Clave secreta híbrida (`ml_kem_pk || ml_kem_sk || x25519_sk`).
    pub fn secret_key(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HYBRID_SECRET_KEY_BYTES);
        bytes.extend_from_slice(self.ml_kem.public_key());
        bytes.extend_from_slice(self.ml_kem.private_key());
        bytes.extend_from_slice(&self.x25519_secret);
        bytes
    }

    /// Reconstruye el par desde [`HybridKeyPair::secret_key`].
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el tamaño o las claves no son
    ///     válidos.
    pub fn from_secret_key(bytes: &[u8]) -> Result<Self, RuscaError> {
        if bytes.len() != HYBRID_SECRET_KEY_BYTES {
            return Err(RuscaError::InvalidConfig(format!(
                "clave híbrida de {} bytes; se esperaban {HYBRID_SECRET_KEY_BYTES}",
                bytes.len()
            )));
        }
        let (public_key, rest) = bytes.split_at(ML_KEM_768_PUBLIC_KEY_BYTES);
        let (private_key, x25519) = rest.split_at(ML_KEM_768_PRIVATE_KEY_BYTES);
        let ml_kem = MlKemKeyPair::from_parts(public_key, private_key)?;
        let x25519_secret: [u8; 32] = x25519.try_into().map_err(|_| {
            RuscaError::InvalidConfig("clave X25519 híbrida mal formada".to_string())
        })?;
        Ok(Self {
            ml_kem,
            x25519_secret,
        })
    }
}

/// Operaciones del KEM híbrido X25519 + ML-KEM-768.
#[derive(Debug, Clone, Copy, Default)]
pub struct HybridKem;

impl HybridKem {
    /// Genera un par híbrido con entropía del SO.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si falta entropía.
    pub fn keygen() -> Result<HybridKeyPair, RuscaError> {
        let ml_kem = MlKem::keygen()?;
        let mut x25519_secret = [0u8; 32];
        getrandom::fill(&mut x25519_secret).map_err(|error| {
            RuscaError::InvalidConfig(format!(
                "entropía del SO no disponible para X25519: {error}"
            ))
        })?;
        Ok(HybridKeyPair {
            ml_kem,
            x25519_secret,
        })
    }

    /// Encapsula un secreto híbrido hacia una clave pública híbrida.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la clave o la entropía fallan.
    pub fn encapsulate(
        public_key: &[u8],
    ) -> Result<(Vec<u8>, [u8; SHARED_SECRET_BYTES]), RuscaError> {
        if public_key.len() != HYBRID_PUBLIC_KEY_BYTES {
            return Err(RuscaError::InvalidConfig(format!(
                "clave pública híbrida de {} bytes; se esperaban {HYBRID_PUBLIC_KEY_BYTES}",
                public_key.len()
            )));
        }
        let (ml_kem_public, x25519_public) = public_key.split_at(ML_KEM_768_PUBLIC_KEY_BYTES);
        let x25519_public: [u8; 32] = x25519_public.try_into().map_err(|_| {
            RuscaError::InvalidConfig("clave pública X25519 mal formada".to_string())
        })?;
        let recipient = X25519PublicKey::from(x25519_public);
        let mut ephemeral_secret = [0u8; 32];
        getrandom::fill(&mut ephemeral_secret).map_err(|error| {
            RuscaError::InvalidConfig(format!(
                "entropía del SO no disponible para X25519 efímero: {error}"
            ))
        })?;
        let ephemeral = StaticSecret::from(ephemeral_secret);
        let ephemeral_public = X25519PublicKey::from(&ephemeral);
        let x25519_shared = ephemeral.diffie_hellman(&recipient);
        let (ml_kem_ciphertext, ml_kem_shared) = MlKem::encapsulate(ml_kem_public)?;
        let shared = combine(&ml_kem_shared, x25519_shared.as_bytes());
        let mut ciphertext = Vec::with_capacity(HYBRID_CIPHERTEXT_BYTES);
        ciphertext.extend_from_slice(&ml_kem_ciphertext);
        ciphertext.extend_from_slice(ephemeral_public.as_bytes());
        Ok((ciphertext, shared))
    }

    /// Desencapsula el secreto híbrido.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si las claves/ciphertext están mal
    ///     formados.
    pub fn decapsulate(
        secret_key: &[u8],
        ciphertext: &[u8],
    ) -> Result<[u8; SHARED_SECRET_BYTES], RuscaError> {
        if ciphertext.len() != HYBRID_CIPHERTEXT_BYTES {
            return Err(RuscaError::InvalidConfig(format!(
                "ciphertext híbrido de {} bytes; se esperaban {HYBRID_CIPHERTEXT_BYTES}",
                ciphertext.len()
            )));
        }
        let pair = HybridKeyPair::from_secret_key(secret_key)?;
        let (ml_kem_ciphertext, ephemeral_public) =
            ciphertext.split_at(ML_KEM_768_CIPHERTEXT_BYTES);
        let ephemeral_public: [u8; 32] = ephemeral_public.try_into().map_err(|_| {
            RuscaError::InvalidConfig("clave efímera X25519 mal formada".to_string())
        })?;
        let ml_kem_shared = MlKem::decapsulate(pair.ml_kem.private_key(), ml_kem_ciphertext)?;
        let x25519_shared = StaticSecret::from(pair.x25519_secret)
            .diffie_hellman(&X25519PublicKey::from(ephemeral_public));
        Ok(combine(&ml_kem_shared, x25519_shared.as_bytes()))
    }
}

/// Combina los secretos ML-KEM y X25519 con SHA-256.
fn combine(ml_kem: &[u8; SHARED_SECRET_BYTES], x25519: &[u8; 32]) -> [u8; SHARED_SECRET_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(ml_kem);
    hasher.update(x25519);
    hasher.finalize().into()
}
