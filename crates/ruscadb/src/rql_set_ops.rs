#![allow(missing_docs)]
#![allow(dead_code)]
//! RQL set operations (SPEC-0164): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct RqlSetOps;

impl RqlSetOps {
    pub fn new() -> Self {
        Self
    }
    pub fn union(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        let mut v: Vec<i64> = a.to_vec();
        for x in b {
            if !v.contains(x) {
                v.push(*x);
            }
        }
        v
    }
    pub fn union_all(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        let mut v = a.to_vec();
        v.extend_from_slice(b);
        v
    }
    pub fn intersect(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        a.iter().filter(|x| b.contains(x)).copied().collect()
    }
    pub fn except(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        a.iter().filter(|x| !b.contains(x)).copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0164-01
    fn test_ac_0164_01_union() {
        let s = RqlSetOps::new();
        assert_eq!(s.union(&[1, 2], &[2, 3]), vec![1, 2, 3]);
    }
    #[test] // @spec AC-0164-02
    fn test_ac_0164_02_union_all() {
        let s = RqlSetOps::new();
        assert_eq!(s.union_all(&[1, 2], &[2, 3]), vec![1, 2, 2, 3]);
    }
    #[test] // @spec AC-0164-03
    fn test_ac_0164_03_intersect() {
        let s = RqlSetOps::new();
        assert_eq!(s.intersect(&[1, 2, 3], &[2, 3, 4]), vec![2, 3]);
    }
    #[test] // @spec AC-0164-04
    fn test_ac_0164_04_except() {
        let s = RqlSetOps::new();
        assert_eq!(s.except(&[1, 2, 3], &[2]), vec![1, 3]);
    }
}
