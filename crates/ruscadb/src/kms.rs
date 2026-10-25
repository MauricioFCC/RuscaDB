#![allow(missing_docs)]
#![allow(dead_code)]
//! KMS/HSM externo (SPEC-0143): stub.

#[derive(Debug, Clone, Default)]
pub struct KmsProvider {
    key: Vec<u8>,
}

impl KmsProvider {
    pub fn new(key: Vec<u8>) -> Self {
        Self { key }
    }
    pub fn wrap(&self, plaintext: &[u8]) -> Vec<u8> {
        plaintext
            .iter()
            .zip(self.key.iter().cycle())
            .map(|(a, b)| a ^ b)
            .collect()
    }
    pub fn unwrap(&self, ciphertext: &[u8]) -> Vec<u8> {
        ciphertext
            .iter()
            .zip(self.key.iter().cycle())
            .map(|(a, b)| a ^ b)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0143-01
    fn test_ac_0143_01_kms_roundtrip() {
        let provider = KmsProvider::new(vec![0xAB, 0xCD]);
        let plaintext = b"secret key";
        let wrapped = provider.wrap(plaintext);
        let unwrapped = provider.unwrap(&wrapped);
        assert_eq!(unwrapped, plaintext);
    }
}
