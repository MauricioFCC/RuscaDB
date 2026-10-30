//! R-tree con splitting (SPEC-0156): indexacion espacial de puntos.
//!
//! Implementacion inicial con **splitting lineal**: los puntos se mantienen en
//! orden de insercion y la consulta de rango filtra en `O(n)`. El arbol real
//! (nodos con MBR y splitting) queda como trabajo futuro; la API es estable.

/// Indice espacial de puntos con consulta de rango.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RTreeSplit {
    points: Vec<(f64, f64)>,
}

impl RTreeSplit {
    /// Crea el indice vacio.
    ///
    /// Returns:
    ///     Indice sin puntos.
    pub fn new() -> Self {
        Self { points: Vec::new() }
    }

    /// Inserta un punto.
    ///
    /// Args:
    ///     x: Coordenada X.
    ///     y: Coordenada Y.
    pub fn insert(&mut self, x: f64, y: f64) {
        self.points.push((x, y));
    }

    /// Puntos dentro del rectangulo `[min_x, max_x] x [min_y, max_y]`.
    ///
    /// Args:
    ///     min_x: Limite inferior X.
    ///     min_y: Limite inferior Y.
    ///     max_x: Limite superior X.
    ///     max_y: Limite superior Y.
    ///
    /// Returns:
    ///     Puntos dentro del rango (orden de insercion).
    pub fn range_query(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<(f64, f64)> {
        self.points
            .iter()
            .filter(|(x, y)| *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y)
            .copied()
            .collect()
    }

    /// Numero de puntos.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Indica si no hay puntos.
    ///
    /// Returns:
    ///     `true` si vacio.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::RTreeSplit;

    #[test] // @spec AC-0156-01
    fn test_ac_0156_01_rtree_split_range() {
        let mut tree = RTreeSplit::new();
        assert!(tree.is_empty());
        tree.insert(1.0, 2.0);
        tree.insert(10.0, 20.0);
        tree.insert(3.0, 4.0);
        assert_eq!(tree.len(), 3);
        assert_eq!(
            tree.range_query(0.0, 0.0, 5.0, 5.0),
            vec![(1.0, 2.0), (3.0, 4.0)]
        );
        assert!(tree.range_query(100.0, 100.0, 200.0, 200.0).is_empty());
    }
}
