#![allow(missing_docs)]
#![allow(dead_code)]
//! RQL window functions (SPEC-0165): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct RqlWindowFuncs;

impl RqlWindowFuncs {
    pub fn new() -> Self {
        Self
    }
    pub fn row_number(&self, n: usize) -> Vec<usize> {
        (1..=n).collect()
    }
    pub fn rank(&self, values: &[i64]) -> Vec<usize> {
        let mut r = vec![1usize; values.len()];
        for i in 1..values.len() {
            r[i] = if values[i] != values[i - 1] {
                i + 1
            } else {
                r[i - 1]
            };
        }
        r
    }
    pub fn lag(&self, values: &[i64], offset: usize) -> Vec<Option<i64>> {
        values
            .iter()
            .enumerate()
            .map(|(i, _)| {
                if i >= offset {
                    Some(values[i - offset])
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn sum_over(&self, values: &[i64]) -> i64 {
        values.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0165-01
    fn test_ac_0165_01_row_number() {
        let w = RqlWindowFuncs::new();
        assert_eq!(w.row_number(3), vec![1, 2, 3]);
    }
    #[test] // @spec AC-0165-02
    fn test_ac_0165_02_sum_over() {
        let w = RqlWindowFuncs::new();
        assert_eq!(w.sum_over(&[1, 2, 3]), 6);
    }
    #[test] // @spec AC-0165-03
    fn test_ac_0165_03_lag() {
        let w = RqlWindowFuncs::new();
        assert_eq!(w.lag(&[10, 20, 30], 1), vec![None, Some(10), Some(20)]);
    }
}
