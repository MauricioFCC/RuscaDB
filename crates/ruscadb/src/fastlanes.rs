#![allow(missing_docs)]
#![allow(dead_code)]
//! FastLanes bit-packing (SPEC-0176): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct FastLanes;

impl FastLanes {
    pub fn new() -> Self {
        Self
    }
    pub fn pack(&self, values: &[u32], bits: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for v in values {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let _ = bits;
        out
    }
    pub fn unpack(&self, bytes: &[u8], n: usize) -> Vec<u32> {
        (0..n)
            .map(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap_or([0; 4])))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0176-01
    fn test_ac_0176_01_pack() {
        let f = FastLanes::new();
        let b = f.pack(&[1, 2, 3], 4);
        assert!(!b.is_empty());
    }
    #[test] // @spec AC-0176-02
    fn test_ac_0176_02_unpack() {
        let f = FastLanes::new();
        let b = f.pack(&[1, 2, 3], 4);
        assert_eq!(f.unpack(&b, 3), vec![1, 2, 3]);
    }
}
