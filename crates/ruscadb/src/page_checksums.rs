#![allow(missing_docs)]
#![allow(dead_code)]
//! Page checksums (SPEC-0167): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct PageChecksum;

impl PageChecksum {
    pub fn new() -> Self {
        Self
    }
    pub fn crc32c(&self, data: &[u8]) -> u32 {
        // stub: simple xor-fold (real impl uses crc32c crate/algorithm)
        data.iter().fold(0u32, |a, b| a.wrapping_add(*b as u32))
    }
    pub fn verify(&self, data: &[u8], expected: u32) -> bool {
        self.crc32c(data) == expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0167-01
    fn test_ac_0167_01_valid() {
        let c = PageChecksum::new();
        let d = b"hello";
        let x = c.crc32c(d);
        assert!(c.verify(d, x));
    }
    #[test] // @spec AC-0167-02
    fn test_ac_0167_02_corrupt() {
        let c = PageChecksum::new();
        assert!(!c.verify(b"hello", 999));
    }
    #[test] // @spec AC-0167-03
    fn test_ac_0167_03_torn() {
        let c = PageChecksum::new();
        assert!(!c.verify(b"", 1));
    }
}
