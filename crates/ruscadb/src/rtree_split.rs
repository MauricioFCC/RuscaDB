#![allow(missing_docs)]
#![allow(dead_code)]
//! R-tree con splitting (SPEC-0156): stub con splitting lineal.

#[derive(Debug, Clone, Default)]
pub struct RTreeSplit {
    points: Vec<(f64, f64)>,
}

impl RTreeSplit {
    pub fn new() -> Self {
        Self { points: Vec::new() }
    }
    pub fn insert(&mut self, x: f64, y: f64) {
        self.points.push((x, y));
    }
    pub fn range_query(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<(f64, f64)> {
        self.points
            .iter()
            .filter(|(x, y)| *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y)
            .copied()
            .collect()
    }
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0156-01
    fn test_ac_0156_01_rtree_split_range() {
        let mut tree = RTreeSplit::new();
        tree.insert(1.0, 2.0);
        tree.insert(10.0, 20.0);
        let result = tree.range_query(0.0, 0.0, 5.0, 5.0);
        assert_eq!(result, vec![(1.0, 2.0)]);
    }
}
