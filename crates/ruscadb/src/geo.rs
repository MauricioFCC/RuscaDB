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

    /// Distancia de Manhattan a otro punto.
    pub fn manhattan(&self, other: &Self) -> f64 {
        (self.x - other.x).abs() + (self.y - other.y).abs()
    }

    /// Punto medio entre dos puntos.
    pub fn midpoint(&self, other: &Self) -> Self {
        Self {
            x: (self.x + other.x) / 2.0,
            y: (self.y + other.y) / 2.0,
        }
    }
}

/// Rectángulo axis-aligned.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct Rect {
    /// Esquina inferior izquierda.
    pub min: Point,
    /// Esquina superior derecha.
    pub max: Point,
}

#[allow(dead_code)]
impl Rect {
    /// Crea un rectángulo a partir de dos esquinas.
    pub fn new(min: Point, max: Point) -> Self {
        Self { min, max }
    }

    /// Indica si un punto está dentro del rectángulo (inclusive).
    pub fn contains(&self, point: &Point) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Área del rectángulo.
    pub fn area(&self) -> f64 {
        (self.max.x - self.min.x).abs() * (self.max.y - self.min.y).abs()
    }

    /// Indica si dos rectángulos se intersectan.
    pub fn intersects(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
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
        assert!((a.manhattan(&b) - 7.0).abs() < 1e-9);
        let mid = a.midpoint(&b);
        assert!((mid.x - 1.5).abs() < 1e-9);
        assert!((mid.y - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_geo_rect_contains() {
        let r = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert!(r.contains(&Point::new(5.0, 5.0)));
        assert!(!r.contains(&Point::new(15.0, 5.0)));
        assert!((r.area() - 100.0).abs() < 1e-9);
        let r2 = Rect::new(Point::new(5.0, 5.0), Point::new(15.0, 15.0));
        assert!(r.intersects(&r2));
        let r3 = Rect::new(Point::new(20.0, 20.0), Point::new(30.0, 30.0));
        assert!(!r.intersects(&r3));
    }
}
