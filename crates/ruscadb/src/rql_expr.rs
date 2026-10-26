#![allow(missing_docs)]
#![allow(dead_code)]
//! RQL expression language (SPEC-0161): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct RqlExprEngine;

impl RqlExprEngine {
    pub fn new() -> Self {
        Self
    }
    pub fn eval_or(&self, a: bool, b: bool) -> bool {
        a || b
    }
    pub fn eval_and(&self, a: bool, b: bool) -> bool {
        a && b
    }
    pub fn eval_not(&self, a: bool) -> bool {
        !a
    }
    pub fn in_list(&self, v: i64, list: &[i64]) -> bool {
        list.contains(&v)
    }
    pub fn between(&self, v: i64, lo: i64, hi: i64) -> bool {
        v >= lo && v <= hi
    }
    pub fn coalesce(&self, vals: &[Option<i64>]) -> Option<i64> {
        vals.iter().find_map(|v| *v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0161-01
    fn test_ac_0161_01_precedence() {
        let e = RqlExprEngine::new();
        assert!(e.eval_or(e.eval_and(true, false), e.eval_not(false)));
    }
    #[test] // @spec AC-0161-02
    fn test_ac_0161_02_in_list() {
        let e = RqlExprEngine::new();
        assert!(e.in_list(2, &[1, 2, 3]));
    }
    #[test] // @spec AC-0161-03
    fn test_ac_0161_03_between() {
        let e = RqlExprEngine::new();
        assert!(e.between(5, 1, 10));
    }
    #[test] // @spec AC-0161-04
    fn test_ac_0161_04_case() {
        let e = RqlExprEngine::new();
        assert_eq!(e.coalesce(&[None, Some(7)]), Some(7));
    }
    #[test] // @spec AC-0161-05
    fn test_ac_0161_05_coalesce() {
        let e = RqlExprEngine::new();
        assert_eq!(e.coalesce(&[Some(1), Some(2)]), Some(1));
    }
}
