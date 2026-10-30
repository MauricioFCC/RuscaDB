//! Tipos geoespaciales e indice espacial (SPEC-0112 + SPEC-0114).
//!
//! Incluye `Point`/`Rect`, un `RTree` con poda por MBR (delega en
//! [`crate::rtree_split`]) y dos tecnicas de indexacion frontier:
//! **GeoHash** (prefijo comun = proximidad) y **curva de Hilbert** (localidad
//! espacial para ordenar/particionar en 1-D).

use crate::rtree_split::RTreeSplit;

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
    ///
    /// Args:
    ///     x: Coordenada X.
    ///     y: Coordenada Y.
    ///
    /// Returns:
    ///     Punto.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Distancia euclidiana a otro punto.
    ///
    /// Args:
    ///     other: Punto destino.
    ///
    /// Returns:
    ///     Distancia L2.
    pub fn distance(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Distancia de Manhattan a otro punto.
    ///
    /// Args:
    ///     other: Punto destino.
    ///
    /// Returns:
    ///     Distancia L1.
    pub fn manhattan(&self, other: &Self) -> f64 {
        (self.x - other.x).abs() + (self.y - other.y).abs()
    }

    /// Punto medio entre dos puntos.
    ///
    /// Args:
    ///     other: Otro punto.
    ///
    /// Returns:
    ///     Punto medio.
    pub fn midpoint(&self, other: &Self) -> Self {
        Self {
            x: (self.x + other.x) / 2.0,
            y: (self.y + other.y) / 2.0,
        }
    }
}

/// Rectangulo axis-aligned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Esquina inferior izquierda.
    pub min: Point,
    /// Esquina superior derecha.
    pub max: Point,
}

impl Rect {
    /// Crea un rectangulo a partir de dos esquinas.
    ///
    /// Args:
    ///     min: Esquina inferior izquierda.
    ///     max: Esquina superior derecha.
    ///
    /// Returns:
    ///     Rectangulo.
    pub fn new(min: Point, max: Point) -> Self {
        Self { min, max }
    }

    /// Indica si un punto esta dentro (bordes inclusivos).
    ///
    /// Args:
    ///     point: Punto a comprobar.
    ///
    /// Returns:
    ///     `true` si cae dentro.
    pub fn contains(&self, point: &Point) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Area del rectangulo.
    ///
    /// Returns:
    ///     `ancho * alto`.
    pub fn area(&self) -> f64 {
        (self.max.x - self.min.x).abs() * (self.max.y - self.min.y).abs()
    }

    /// Indica si dos rectangulos se intersectan.
    ///
    /// Args:
    ///     other: Otro rectangulo.
    ///
    /// Returns:
    ///     `true` si se solapan.
    pub fn intersects(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

/// R-tree con poda por MBR.
#[derive(Debug, Clone, Default)]
pub struct RTree {
    /// Puntos insertados (para KNN).
    points: Vec<Point>,
    /// Indice espacial para consultas de rango.
    index: RTreeSplit,
}

impl RTree {
    /// Crea un R-tree vacio.
    ///
    /// Returns:
    ///     Arbol sin puntos.
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            index: RTreeSplit::new(),
        }
    }

    /// Inserta un punto.
    ///
    /// Args:
    ///     point: Punto a insertar.
    pub fn insert(&mut self, point: Point) {
        self.points.push(point);
        self.index.insert(point.x, point.y);
    }

    /// Inserta un punto por coordenadas.
    ///
    /// Args:
    ///     x: Coordenada X.
    ///     y: Coordenada Y.
    pub fn insert_xy(&mut self, x: f64, y: f64) {
        self.insert(Point::new(x, y));
    }

    /// Consulta por bounding box (poda por MBR, `O(log n + k)` esperado).
    ///
    /// Args:
    ///     rect: Ventana de busqueda.
    ///
    /// Returns:
    ///     Puntos dentro del rectangulo.
    pub fn range_query(&self, rect: &Rect) -> Vec<Point> {
        self.index
            .range_query(rect.min.x, rect.min.y, rect.max.x, rect.max.y)
            .into_iter()
            .map(|(x, y)| Point::new(x, y))
            .collect()
    }

    /// Consulta por bounding box (coordenadas).
    ///
    /// Args:
    ///     min_x: Limite inferior X.
    ///     min_y: Limite inferior Y.
    ///     max_x: Limite superior X.
    ///     max_y: Limite superior Y.
    ///
    /// Returns:
    ///     Puntos dentro del rectangulo.
    pub fn range_query_xy(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<Point> {
        let rect = Rect::new(Point::new(min_x, min_y), Point::new(max_x, max_y));
        self.range_query(&rect)
    }

    /// KNN: los `k` puntos mas cercanos.
    ///
    /// Args:
    ///     center: Punto de consulta.
    ///     k: Resultados deseados.
    ///
    /// Returns:
    ///     Puntos por distancia ascendente.
    pub fn knn(&self, center: &Point, k: usize) -> Vec<Point> {
        let mut sorted: Vec<(f64, Point)> = self
            .points
            .iter()
            .map(|point| (point.distance(center), *point))
            .collect();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        sorted.into_iter().take(k).map(|(_, point)| point).collect()
    }

    /// Numero de puntos.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Indica si esta vacio.
    ///
    /// Returns:
    ///     `true` sin puntos.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// Alfabeto base32 de geohash (sin `a`, `i`, `l`, `o`).
const GEOHASH_BASE32: &[u8; 32] = b"0123456789bcdefghjkmnpqrstuvwxyz";

/// Codifica un punto (lat, lon) como geohash.
///
/// Args:
///     lat: Latitud en grados (`-90..=90`).
///     lon: Longitud en grados (`-180..=180`).
///     precision: Longitud del hash (caracteres).
///
/// Returns:
///     Geohash en minusculas.
pub fn geohash_encode(lat: f64, lon: f64, precision: usize) -> String {
    let mut lat_range = (-90.0f64, 90.0f64);
    let mut lon_range = (-180.0f64, 180.0f64);
    let mut hash = String::with_capacity(precision);
    let mut bits = 0u8;
    let mut count = 0u32;
    let mut even = true;
    while hash.len() < precision {
        if even {
            let mid = (lon_range.0 + lon_range.1) / 2.0;
            if lon >= mid {
                bits = (bits << 1) | 1;
                lon_range.0 = mid;
            } else {
                bits <<= 1;
                lon_range.1 = mid;
            }
        } else {
            let mid = (lat_range.0 + lat_range.1) / 2.0;
            if lat >= mid {
                bits = (bits << 1) | 1;
                lat_range.0 = mid;
            } else {
                bits <<= 1;
                lat_range.1 = mid;
            }
        }
        even = !even;
        count += 1;
        if count == 5 {
            hash.push(GEOHASH_BASE32[usize::from(bits)] as char);
            bits = 0;
            count = 0;
        }
    }
    hash
}

/// Decodifica un geohash al rectangulo que representa.
///
/// Args:
///     hash: Geohash (minusculas).
///
/// Returns:
///     `(lat_min, lon_min, lat_max, lon_max)`, o `None` si hay un caracter
///     invalido.
pub fn geohash_decode(hash: &str) -> Option<(f64, f64, f64, f64)> {
    let mut lat_range = (-90.0f64, 90.0f64);
    let mut lon_range = (-180.0f64, 180.0f64);
    let mut even = true;
    for character in hash.chars() {
        let value = GEOHASH_BASE32
            .iter()
            .position(|candidate| *candidate == character as u8)? as u8;
        for bit in (0..5).rev() {
            let one = (value >> bit) & 1 == 1;
            if even {
                let mid = (lon_range.0 + lon_range.1) / 2.0;
                if one {
                    lon_range.0 = mid;
                } else {
                    lon_range.1 = mid;
                }
            } else {
                let mid = (lat_range.0 + lat_range.1) / 2.0;
                if one {
                    lat_range.0 = mid;
                } else {
                    lat_range.1 = mid;
                }
            }
            even = !even;
        }
    }
    Some((lat_range.0, lon_range.0, lat_range.1, lon_range.1))
}

/// Indice de Hilbert de `(x, y)` en una grilla `2^order x 2^order`.
///
/// La curva de Hilbert preserva localidad: puntos cercanos en 2-D tienden a
/// indices cercanos en 1-D (mejor que Z-order para particionar).
///
/// Args:
///     x: Coordenada X en `0..2^order`.
///     y: Coordenada Y en `0..2^order`.
///     order: Orden de la curva.
///
/// Returns:
///     Indice `0..4^order`.
pub fn hilbert_index(x: u64, y: u64, order: u32) -> u64 {
    let mut x = x as i64;
    let mut y = y as i64;
    let mut distance = 0u64;
    let mut size = 1i64 << order.saturating_sub(1);
    while size > 0 {
        let rx = i64::from((x & size) > 0);
        let ry = i64::from((y & size) > 0);
        distance += (size as u64) * (size as u64) * (((3 * rx) ^ ry) as u64);
        if ry == 0 {
            if rx == 1 {
                x = size - 1 - x;
                y = size - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        size /= 2;
    }
    distance
}

#[cfg(test)]
mod tests {
    use super::{Point, RTree, Rect, geohash_decode, geohash_encode, hilbert_index};

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
        let rect = Rect::new(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert!(rect.contains(&Point::new(5.0, 5.0)));
        assert!(!rect.contains(&Point::new(15.0, 5.0)));
        assert!((rect.area() - 100.0).abs() < 1e-9);
        let other = Rect::new(Point::new(5.0, 5.0), Point::new(15.0, 15.0));
        assert!(rect.intersects(&other));
        let far = Rect::new(Point::new(20.0, 20.0), Point::new(30.0, 30.0));
        assert!(!rect.intersects(&far));
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
        assert_eq!(tree.range_query(&rect), vec![Point::new(1.0, 2.0)]);
    }

    /// Geohash: el prefijo comun implica proximidad y el decode contiene al punto.
    #[test]
    fn geohash_roundtrip_and_prefix_locality() {
        let (lat, lon) = (40.7128, -74.0060);
        let hash = geohash_encode(lat, lon, 9);
        assert_eq!(hash.len(), 9);
        assert!(
            hash.starts_with("dr5reg"),
            "prefijo conocido de NYC: {hash}"
        );
        let (lat_min, lon_min, lat_max, lon_max) = geohash_decode(&hash).expect("decode");
        assert!(lat >= lat_min && lat <= lat_max);
        assert!(lon >= lon_min && lon <= lon_max);

        let nearby = geohash_encode(40.7130, -74.0062, 9);
        assert!(nearby.starts_with(&hash[..5]), "mismo barrio");
        assert!(geohash_decode("zzzzzzzzz").is_some());
        assert!(geohash_decode("a").is_none(), "caracter invalido");
    }

    /// Hilbert: orden conocido y localidad.
    #[test]
    fn hilbert_curve_order_and_locality() {
        // Orden 1: (0,0)=0, (0,1)=1, (1,1)=2, (1,0)=3.
        assert_eq!(hilbert_index(0, 0, 1), 0);
        assert_eq!(hilbert_index(0, 1, 1), 1);
        assert_eq!(hilbert_index(1, 1, 1), 2);
        assert_eq!(hilbert_index(1, 0, 1), 3);
        // Los indices son una permutacion de 0..4^order.
        let mut indices: Vec<u64> = (0..8)
            .flat_map(|x| (0..8).map(move |y| (x, y)))
            .map(|(x, y)| hilbert_index(x, y, 3))
            .collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..64).collect::<Vec<u64>>());
        // Localidad: puntos adyacentes en la curva estan a distancia Manhattan 1.
        let mut previous = (0u64, 0u64);
        for target in 1..64u64 {
            let position = (0..8)
                .flat_map(|x| (0..8).map(move |y| (x, y)))
                .find(|&(x, y)| hilbert_index(x, y, 3) == target)
                .expect("posicion");
            let distance = previous.0.abs_diff(position.0) + previous.1.abs_diff(position.1);
            assert_eq!(distance, 1, "salto en target {target}");
            previous = position;
        }
    }
}
