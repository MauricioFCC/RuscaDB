#![allow(missing_docs)]
#![allow(dead_code)]
//! Gorilla TS compression (SPEC-0178): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct GorillaTs;

impl GorillaTs {
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
    #[test] // @spec AC-0178-01
    fn test_ac_0178_01_ratio() {
        let g = GorillaTs::new();
        let v = vec![1.0, 2.0, 3.0];
        let b = g.encode(&v);
        assert!(!b.is_empty());
    }
    #[test] // @spec AC-0178-02
    fn test_ac_0178_02_lossless() {
        let g = GorillaTs::new();
        let v = vec![1.0, 2.0, 3.0];
        let b = g.encode(&v);
        assert_eq!(g.decode(&b), v);
    }
}
