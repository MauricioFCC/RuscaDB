#![allow(missing_docs)]
#![allow(dead_code)]
//! Reordenamiento de joins (SPEC-0153): cost-based.

#[derive(Debug, Clone, Copy, Default)]
pub struct JoinReorderer;

impl JoinReorderer {
    /// Reordena dos tablas por tamano (mas pequena primero).
    pub fn reorder(left_rows: usize, right_rows: usize) -> bool {
        // Returns true if swap is needed (left should go second).
        left_rows > right_rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0153-01
    fn test_ac_0153_01_join_reorder() {
        assert!(JoinReorderer::reorder(100, 10));
        assert!(!JoinReorderer::reorder(10, 100));
    }
}
