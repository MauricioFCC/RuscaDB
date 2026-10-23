//! Indice espacial R-tree opcional (SPEC-0114): stub sin dependencias externas.

/// Arbol R simple (stub lineal).
#[derive(Debug, Clone)]
pub struct RTree {
    /// Puntos insertados.
    points: Vec<(f64, f64)>,
}

impl Default for RTree {
    fn default() -> Self {
        Self::new()
    }
}

impl RTree {
    /// Crea un R-tree vacio.
    pub fn new() -> Self {
        Self { points: Vec::new() }
    }

    /// Inserta un punto (x, y).
    pub fn insert(&mut self, x: f64, y: f64) {
        self.points.push((x, y));
    }

    /// Consulta por bounding box.
    pub fn range_query(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<(f64, f64)> {
        self.points
            .iter()
            .filter(|(x, y)| *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y)
            .copied()
            .collect()
    }

    /// Numero de puntos.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Indica si esta vacio.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0114-01
    fn test_ac_0114_01_rtree_range_query() {
        let mut tree = RTree::new();
        tree.insert(1.0, 2.0);
        tree.insert(10.0, 20.0);
        let result = tree.range_query(0.0, 0.0, 5.0, 5.0);
        assert_eq!(result, vec![(1.0, 2.0)]);
    }
}
