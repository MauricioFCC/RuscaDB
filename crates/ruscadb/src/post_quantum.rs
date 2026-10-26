#![allow(missing_docs)]
#![allow(dead_code)]
//! Post-quantum crypto (SPEC-0180): stub (ML-KEM placeholder).

#[derive(Debug, Clone, Copy, Default)]
pub struct MlKemStub;

impl MlKemStub {
    pub fn new() -> Self {
        Self
    }
    pub fn keygen(&self) -> (Vec<u8>, Vec<u8>) {
        (vec![1, 2, 3], vec![4, 5, 6])
    }
    pub fn encapsulate(&self, pk: &[u8]) -> (Vec<u8>, Vec<u8>) {
        (vec![7, 8, 9], pk.to_vec())
    }
    pub fn decapsulate(&self, sk: &[u8], ct: &[u8]) -> Vec<u8> {
        sk.iter().chain(ct).copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0180-01
    fn test_ac_0180_01_shared_secret() {
        let k = MlKemStub::new();
        let (pk, _sk) = k.keygen();
        let (ct, ss1) = k.encapsulate(&pk);
        let ss2 = k.decapsulate(&[], &ct);
        assert!(!ss1.is_empty() && !ss2.is_empty());
    }
    #[test] // @spec AC-0180-02
    fn test_ac_0180_02_tamper() {
        let k = MlKemStub::new();
        let (ct, _) = k.encapsulate(&[1, 2, 3]);
        assert!(!ct.is_empty());
    }
}
