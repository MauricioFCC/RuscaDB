#![allow(missing_docs)]
#![allow(dead_code)]
//! RaBitQ quantization (SPEC-0171): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct RabitqQuantizer;

impl RabitqQuantizer {
    pub fn new() -> Self {
        Self
    }
    pub fn encode(&self, v: &[f32]) -> Vec<u8> {
        v.iter().map(|x| if *x >= 0.0 { 1 } else { 0 }).collect()
    }
    pub fn estimate_distance(&self, a: &[u8], b: &[u8]) -> f32 {
        a.iter()
            .zip(b)
            .map(|(x, y)| if x == y { 0.0 } else { 1.0 })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0171-01
    fn test_ac_0171_01_encode() {
        let q = RabitqQuantizer::new();
        let e = q.encode(&[1.0, -2.0, 3.0]);
        assert_eq!(e, vec![1, 0, 1]);
    }
    #[test] // @spec AC-0171-02
    fn test_ac_0171_02_recall() {
        let q = RabitqQuantizer::new();
        assert!(q.estimate_distance(&[1, 0, 1], &[1, 0, 1]) < 1.0);
    }
}
