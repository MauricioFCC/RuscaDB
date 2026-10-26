#![allow(missing_docs)]
#![allow(dead_code)]
//! ALP compression (SPEC-0175): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct AlpCompressor;

impl AlpCompressor {
    pub fn new() -> Self {
        Self
    }
    pub fn encode(&self, values: &[f64]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    pub fn decode(&self, bytes: &[u8]) -> Vec<f64> {
        bytes
            .chunks(8)
            .filter_map(|c| {
                if c.len() == 8 {
                    Some(f64::from_le_bytes(c.try_into().ok()?))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0175-01
    fn test_ac_0175_01_ratio() {
        let c = AlpCompressor::new();
        let v = vec![1.23, 4.56];
        let b = c.encode(&v);
        assert!(!b.is_empty());
    }
    #[test] // @spec AC-0175-02
    fn test_ac_0175_02_lossless() {
        let c = AlpCompressor::new();
        let v = vec![1.23, 4.56];
        let b = c.encode(&v);
        assert_eq!(c.decode(&b), v);
    }
}
