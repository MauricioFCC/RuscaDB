#![allow(missing_docs)]
#![allow(dead_code)]
//! ACORN filtered search (SPEC-0172): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct AcornSearch {
    pub expand_factor: usize,
}

impl AcornSearch {
    pub fn new(expand_factor: usize) -> Self {
        Self { expand_factor }
    }
    pub fn should_expand(&self, selectivity: f32) -> bool {
        selectivity < 0.05
    }
    pub fn search(&self, candidates: usize) -> usize {
        candidates * (self.expand_factor.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0172-01
    fn test_ac_0172_01_recall() {
        let a = AcornSearch::new(4);
        assert!(a.should_expand(0.01));
    }
    #[test] // @spec AC-0172-02
    fn test_ac_0172_02_prefilter() {
        let a = AcornSearch::new(4);
        assert!(!a.should_expand(0.5));
    }
}
