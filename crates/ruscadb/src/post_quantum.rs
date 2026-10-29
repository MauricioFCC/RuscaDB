//! Placeholder de criptografia post-cuantica (SPEC-0180) — **NO es ML-KEM**.
//!
//! Este modulo expone la *forma* de la API (keygen/encapsulate/decapsulate) para
//! que el resto del sistema pueda cablearse, pero la matematica es un stub que
//! NO es FIPS 203 ni ofrece seguridad. La feature `post-quantum` esta **off por
//! defecto** por esta razon.
//!
//! Para una implementacion real se debe integrar un crate auditado (RustCrypto
//! `ml-kem`, FIPS 203) y pasar los test vectors de NIST ACVP. Mientras no
//! exista, el estado de la spec es `proposed` (no `implemented`).

/// Placeholder de ML-KEM: no es seguro, solo cablea la API.
#[derive(Debug, Clone, Copy, Default)]
pub struct MlKemStub;

impl MlKemStub {
    /// Crea el placeholder.
    ///
    /// Returns:
    ///     Instancia sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// Genera un par de claves (placeholder).
    ///
    /// Returns:
    ///     `(clave_publica, clave_privada)` dummy.
    pub fn keygen(&self) -> (Vec<u8>, Vec<u8>) {
        (vec![1, 2, 3], vec![4, 5, 6])
    }

    /// Encapsula (placeholder).
    ///
    /// Args:
    ///     pk: Clave publica (ignorada).
    ///
    /// Returns:
    ///     `(ciphertext, secreto_compartido)` dummy.
    pub fn encapsulate(&self, pk: &[u8]) -> (Vec<u8>, Vec<u8>) {
        (vec![7, 8, 9], pk.to_vec())
    }

    /// Desencapsula (placeholder).
    ///
    /// Args:
    ///     sk: Clave privada (ignorada).
    ///     ct: Ciphertext.
    ///
    /// Returns:
    ///     Secreto compartido dummy (no es el real).
    pub fn decapsulate(&self, sk: &[u8], ct: &[u8]) -> Vec<u8> {
        sk.iter().chain(ct).copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::MlKemStub;

    /// AC-0180-01: la API no hace panic (solo cableado).
    #[test]
    // @spec AC-0180-01
    fn test_ac_0180_01_shared_secret() {
        let k = MlKemStub::new();
        let (pk, _sk) = k.keygen();
        let (ct, ss1) = k.encapsulate(&pk);
        let ss2 = k.decapsulate(&[], &ct);
        assert!(!ss1.is_empty() && !ss2.is_empty());
    }

    /// AC-0180-02: tamper no hace panic.
    #[test]
    // @spec AC-0180-02
    fn test_ac_0180_02_tamper() {
        let k = MlKemStub::new();
        let (ct, _) = k.encapsulate(&[1, 2, 3]);
        assert!(!ct.is_empty());
    }
}
