#![allow(missing_docs)]
#![allow(dead_code)]
//! Persistent HNSW (SPEC-0173): stub.

#[derive(Debug, Clone, Default)]
pub struct PersistentHnsw {
    pub vectors: usize,
    pub version: u32,
}

impl PersistentHnsw {
    pub fn new() -> Self {
        Self {
            vectors: 0,
            version: 1,
        }
    }
    pub fn add(&mut self) {
        self.vectors += 1;
    }
    pub fn save(&self) -> Vec<u8> {
        format!("v{}:{}", self.version, self.vectors).into_bytes()
    }
    pub fn load(bytes: &[u8]) -> Option<Self> {
        let s = std::str::from_utf8(bytes).ok()?;
        let (v, n) = s.split_once(':')?;
        let version: u32 = v.trim_start_matches('v').parse().ok()?;
        let vectors: usize = n.parse().ok()?;
        Some(Self { vectors, version })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0173-01
    fn test_ac_0173_01_roundtrip() {
        let mut h = PersistentHnsw::new();
        h.add();
        h.add();
        let b = h.save();
        let h2 = PersistentHnsw::load(&b).unwrap();
        assert_eq!(h2.vectors, 2);
    }
    #[test] // @spec AC-0173-02
    fn test_ac_0173_02_corrupt() {
        assert!(PersistentHnsw::load(b"garbage").is_none());
    }
}
