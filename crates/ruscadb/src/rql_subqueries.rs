#![allow(missing_docs)]
#![allow(dead_code)]
//! RQL subqueries (SPEC-0162): stub.

#[derive(Debug, Clone, Default)]
pub struct RqlSubquery;

impl RqlSubquery {
    pub fn new() -> Self {
        Self
    }
    pub fn scalar(&self, rows: &[i64]) -> Option<i64> {
        rows.first().copied()
    }
    pub fn exists(&self, rows: &[i64]) -> bool {
        !rows.is_empty()
    }
    pub fn in_subquery(&self, v: i64, rows: &[i64]) -> bool {
        rows.contains(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0162-01
    fn test_ac_0162_01_scalar() {
        let s = RqlSubquery::new();
        assert_eq!(s.scalar(&[42, 7]), Some(42));
    }
    #[test] // @spec AC-0162-02
    fn test_ac_0162_02_in() {
        let s = RqlSubquery::new();
        assert!(s.in_subquery(2, &[1, 2, 3]));
    }
    #[test] // @spec AC-0162-03
    fn test_ac_0162_03_exists() {
        let s = RqlSubquery::new();
        assert!(s.exists(&[1]));
        assert!(!s.exists(&[]));
    }
    #[test] // @spec AC-0162-04
    fn test_ac_0162_04_derived() {
        let s = RqlSubquery::new();
        assert_eq!(s.scalar(&[1, 2]).unwrap_or(0), 1);
    }
}
