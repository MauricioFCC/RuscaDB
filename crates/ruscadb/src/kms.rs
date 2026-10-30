//! KMS/HSM externo (SPEC-0143): **placeholder** de envoltura de claves.
//!
//! El `wrap`/`unwrap` actual es XOR con una clave local: **no es criptografia
//! segura** y no sustituye a un KMS/HSM real. Se mantiene como contrato de API
//! para inyectar el proveedor auditado (misma decision que ML-KEM).

/// Proveedor KMS (placeholder XOR).
#[derive(Debug, Clone, Default)]
pub struct KmsProvider {
    key: Vec<u8>,
}

impl KmsProvider {
    /// Crea el proveedor con una clave local.
    ///
    /// Args:
    ///     key: Clave (se cicla sobre el payload).
    ///
    /// Returns:
    ///     Proveedor placeholder.
    pub fn new(key: Vec<u8>) -> Self {
        Self { key }
    }

    /// "Envuelve" el payload (XOR; no seguro).
    ///
    /// Args:
    ///     plaintext: Bytes originales.
    ///
    /// Returns:
    ///     Bytes transformados.
    pub fn wrap(&self, plaintext: &[u8]) -> Vec<u8> {
        plaintext
            .iter()
            .zip(self.key.iter().cycle())
            .map(|(byte, key)| byte ^ key)
            .collect()
    }

    /// "Desenvuelve" el payload (XOR; inverso de `wrap`).
    ///
    /// Args:
    ///     ciphertext: Bytes envueltos.
    ///
    /// Returns:
    ///     Bytes originales.
    pub fn unwrap(&self, ciphertext: &[u8]) -> Vec<u8> {
        self.wrap(ciphertext)
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
        assert_eq!(provider.unwrap(&wrapped), plaintext);
    }
}
