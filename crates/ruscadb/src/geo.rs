//! Tipos geoespaciales + indice espacial (SPEC-0112 + SPEC-0114): unificado.

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
pub struct Rect {
    /// Esquina inferior izquierda.
    pub min: Point,
    /// Esquina superior derecha.
    pub max: Point,
}

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

/// Árbol R simple (stub lineal) integrado con Point.
#[derive(Debug, Clone, Default)]
pub struct RTree {
    /// Puntos insertados.
    points: Vec<Point>,
}

impl RTree {
    /// Crea un R-tree vacío.
    pub fn new() -> Self {
        Self { points: Vec::new() }
    }

    /// Inserta un punto.
    pub fn insert(&mut self, point: Point) {
        self.points.push(point);
    }

    /// Inserta un punto por coordenadas.
    pub fn insert_xy(&mut self, x: f64, y: f64) {
        self.points.push(Point::new(x, y));
    }

    /// Consulta por bounding box.
    pub fn range_query(&self, rect: &Rect) -> Vec<Point> {
        self.points
            .iter()
            .filter(|p| rect.contains(p))
            .copied()
            .collect()
    }

    /// Consulta por bounding box (coordenadas).
    pub fn range_query_xy(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<Point> {
        let rect = Rect::new(Point::new(min_x, min_y), Point::new(max_x, max_y));
        self.range_query(&rect)
    }

    /// KNN: los k puntos más cercanos a un punto dado.
    pub fn knn(&self, center: &Point, k: usize) -> Vec<Point> {
        let mut sorted: Vec<(f64, Point)> = self
            .points
            .iter()
            .map(|p| (p.distance(center), *p))
            .collect();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        sorted.into_iter().take(k).map(|(_, p)| p).collect()
    }

    /// Número de puntos.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Indica si está vacío.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
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

    #[test]
    fn test_rtree_knn() {
        let mut tree = RTree::new();
        tree.insert(Point::new(0.0, 0.0));
        tree.insert(Point::new(3.0, 4.0));
        tree.insert(Point::new(10.0, 10.0));
        let nearest = tree.knn(&Point::new(0.0, 0.0), 2);
        assert_eq!(nearest.len(), 2);
        assert_eq!(nearest[0], Point::new(0.0, 0.0));
    }

    #[test]
    fn test_rtree_range_query() {
        let mut tree = RTree::new();
        tree.insert_xy(1.0, 2.0);
        tree.insert_xy(10.0, 20.0);
        let rect = Rect::new(Point::new(0.0, 0.0), Point::new(5.0, 5.0));
        let result = tree.range_query(&rect);
        assert_eq!(result, vec![Point::new(1.0, 2.0)]);
    }
}
