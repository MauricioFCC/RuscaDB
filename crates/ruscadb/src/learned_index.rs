#![allow(missing_docs)]
#![allow(dead_code)]
//! Learned index (SPEC-0174): stub.

#[derive(Debug, Clone)]
pub struct LearnedIndex {
    pub epsilon: usize,
    pub segments: Vec<(i64, i64)>,
}

impl LearnedIndex {
    pub fn build(keys: &[i64], epsilon: usize) -> Self {
        let segs = if keys.is_empty() {
            vec![]
        } else {
            vec![(keys[0], keys[keys.len() - 1])]
        };
        Self {
            epsilon,
            segments: segs,
        }
    }
    pub fn predict(&self, _key: i64) -> Option<usize> {
        if self.segments.is_empty() {
            return None;
        }
        Some(0)
    }
    pub fn lookup(&self, keys: &[i64], key: i64) -> Option<usize> {
        keys.iter().position(|&k| k == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0174-01
    fn test_ac_0174_01_build() {
        let idx = LearnedIndex::build(&[1, 2, 3, 4, 5], 2);
        assert!(!idx.segments.is_empty());
    }
    #[test] // @spec AC-0174-02
    fn test_ac_0174_02_lookup() {
        let keys = vec![1, 2, 3];
        let idx = LearnedIndex::build(&keys, 2);
        assert_eq!(idx.lookup(&keys, 2), Some(1));
        assert_eq!(idx.lookup(&keys, 9), None);
    }
}
