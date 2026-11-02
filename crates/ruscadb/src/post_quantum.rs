//! Re-export de ML-KEM FIPS 203 + híbrido X25519 (SPEC-0180).
//!
//! La implementación vive en `ruscadb-crypto::ml_kem` (feature
//! `post-quantum`, OFF por defecto). Esta fachada solo expone los tipos para
//! que los adapters dependan de `ruscadb` y no del crate de criptografía.

pub use ruscadb_crypto::ml_kem::{
    HYBRID_CIPHERTEXT_BYTES, HYBRID_PUBLIC_KEY_BYTES, HYBRID_SECRET_KEY_BYTES, HybridKem,
    HybridKeyPair, ML_KEM_768_CIPHERTEXT_BYTES, ML_KEM_768_PRIVATE_KEY_BYTES,
    ML_KEM_768_PUBLIC_KEY_BYTES, MlKem, MlKemKeyPair, SHARED_SECRET_BYTES,
};
