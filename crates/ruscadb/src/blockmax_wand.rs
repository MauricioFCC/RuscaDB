#![allow(missing_docs)]
#![allow(dead_code)]
//! BlockMax WAND (SPEC-0177): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct BlockMaxWand;

impl BlockMaxWand {
    pub fn new() -> Self {
        Self
    }
    pub fn top_k(&self, scores: &[(u64, f32)], k: usize) -> Vec<(u64, f32)> {
        let mut v = scores.to_vec();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(k);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0177-01
    fn test_ac_0177_01_topk() {
        let w = BlockMaxWand::new();
        let r = w.top_k(&[(1, 0.5), (2, 0.9), (3, 0.1)], 2);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, 2);
    }
    #[test] // @spec AC-0177-02
    fn test_ac_0177_02_prune() {
        let w = BlockMaxWand::new();
        let r = w.top_k(&[(1, 0.5)], 1);
        assert_eq!(r.len(), 1);
    }
}
