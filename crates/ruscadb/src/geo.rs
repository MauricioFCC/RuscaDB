//! Tipos geoespaciales opcionales (SPEC-0112): geometría 2D euclidiana.

/// Punto en el plano.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Coordenada X.
    pub x: f64,
    /// Coordenada Y.
    pub y: f64,
}

impl Point {
    /// Crea un punto.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Distancia euclidiana a otro punto.
    pub fn distance(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0112-01
    fn test_ac_0112_01_geo_distance() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(3.0, 4.0);
        assert!((a.distance(&b) - 5.0).abs() < 1e-9);
    }
}
