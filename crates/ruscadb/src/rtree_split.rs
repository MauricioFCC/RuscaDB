//! R-tree con splitting (SPEC-0156): indexacion espacial con poda por MBR.
//!
//! Implementa un R-tree (Guttman, 1984) con **split por mediana** (se ordenan
//! las entradas por el eje de mayor dispersion y se parte por la mitad). La
//! consulta de rango poda subarboles cuyo MBR no intersecta, con coste
//! `O(log n + k)` esperado en vez del `O(n)` del escaneo lineal.
//!
//! Sin `unsafe` y sin dependencias. Referencias: A. Guttman, "R-Trees: A
//! Dynamic Index Structure for Spatial Searching", SIGMOD 1984; S. Leutenegger
//! et al., "STR: A Simple and Efficient Algorithm for R-Tree Packing", ICDE
//! 1997 (empaquetado por ordenacion).

/// Rectangulo axis-aligned (MBR).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Limite inferior X.
    pub min_x: f64,
    /// Limite inferior Y.
    pub min_y: f64,
    /// Limite superior X.
    pub max_x: f64,
    /// Limite superior Y.
    pub max_y: f64,
}

impl Rect {
    /// Rectangulo degenerado de un punto.
    fn point(x: f64, y: f64) -> Self {
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }

    /// Union de dos MBR.
    fn union(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    /// Area del rectangulo.
    fn area(&self) -> f64 {
        (self.max_x - self.min_x) * (self.max_y - self.min_y)
    }

    /// `true` si los rectangulos se solapan (bordes inclusivos).
    fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// `true` si el punto cae dentro (bordes inclusivos).
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Incremento de area al absorber `other`.
    fn enlargement(&self, other: &Self) -> f64 {
        self.union(*other).area() - self.area()
    }

    /// Centro en X.
    fn center_x(&self) -> f64 {
        (self.min_x + self.max_x) / 2.0
    }

    /// Centro en Y.
    fn center_y(&self) -> f64 {
        (self.min_y + self.max_y) / 2.0
    }
}

/// Nodo del R-tree.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    /// Hoja con puntos.
    Leaf { mbr: Rect, points: Vec<(f64, f64)> },
    /// Nodo interno con hijos.
    Branch { mbr: Rect, children: Vec<Node> },
}

impl Node {
    /// MBR del nodo.
    fn mbr(&self) -> Rect {
        match self {
            Node::Leaf { mbr, .. } | Node::Branch { mbr, .. } => *mbr,
        }
    }

    /// Recalcula el MBR recursivamente desde las hojas.
    fn recompute(&mut self) {
        match self {
            Node::Leaf { mbr, points } => {
                if let Some(first) = points.first() {
                    let mut bounds = Rect::point(first.0, first.1);
                    for &(x, y) in &points[1..] {
                        bounds = bounds.union(Rect::point(x, y));
                    }
                    *mbr = bounds;
                }
            }
            Node::Branch { mbr, children } => {
                for child in children.iter_mut() {
                    child.recompute();
                }
                if let Some(first) = children.first() {
                    let mut bounds = first.mbr();
                    for child in &children[1..] {
                        bounds = bounds.union(child.mbr());
                    }
                    *mbr = bounds;
                }
            }
        }
    }
}

/// R-tree de puntos.
#[derive(Debug, Clone, PartialEq)]
pub struct RTreeSplit {
    root: Option<Node>,
    max_entries: usize,
    len: usize,
}

impl Default for RTreeSplit {
    fn default() -> Self {
        Self::new()
    }
}

impl RTreeSplit {
    /// Crea el arbol con 8 entradas maximas por nodo.
    ///
    /// Returns:
    ///     Arbol vacio.
    pub fn new() -> Self {
        Self::with_max_entries(8)
    }

    /// Crea el arbol con un fanout explicito (minimo 4).
    ///
    /// Args:
    ///     max_entries: Entradas maximas por nodo.
    ///
    /// Returns:
    ///     Arbol vacio.
    pub fn with_max_entries(max_entries: usize) -> Self {
        Self {
            root: None,
            max_entries: max_entries.max(4),
            len: 0,
        }
    }

    /// Inserta un punto.
    ///
    /// Args:
    ///     x: Coordenada X.
    ///     y: Coordenada Y.
    pub fn insert(&mut self, x: f64, y: f64) {
        let point = (x, y);
        match self.root.take() {
            None => {
                self.root = Some(Node::Leaf {
                    mbr: Rect::point(x, y),
                    points: vec![point],
                });
            }
            Some(mut root) => {
                let split = insert_recursive(&mut root, point, self.max_entries);
                if let Some(mut sibling) = split {
                    root.recompute();
                    sibling.recompute();
                    let mbr = root.mbr().union(sibling.mbr());
                    self.root = Some(Node::Branch {
                        mbr,
                        children: vec![root, sibling],
                    });
                } else {
                    self.root = Some(root);
                }
            }
        }
        self.len += 1;
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
    ///     Puntos dentro del rango (sin orden garantizado).
    pub fn range_query(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<(f64, f64)> {
        let window = Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        };
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            query_recursive(root, &window, &mut out);
        }
        out
    }

    /// Numero de puntos.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Indica si no hay puntos.
    ///
    /// Returns:
    ///     `true` si vacio.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Altura del arbol (0 = vacio, 1 = solo raiz).
    ///
    /// Returns:
    ///     Profundidad en niveles.
    pub fn height(&self) -> usize {
        fn depth(node: &Node) -> usize {
            match node {
                Node::Leaf { .. } => 1,
                Node::Branch { children, .. } => 1 + children.iter().map(depth).max().unwrap_or(0),
            }
        }
        self.root.as_ref().map_or(0, depth)
    }

    /// MBR global `(min_x, min_y, max_x, max_y)`.
    ///
    /// Returns:
    ///     `Some(mbr)` o `None` si el arbol esta vacio.
    pub fn mbr(&self) -> Option<(f64, f64, f64, f64)> {
        self.root.as_ref().map(|root| {
            let mbr = root.mbr();
            (mbr.min_x, mbr.min_y, mbr.max_x, mbr.max_y)
        })
    }
}

/// Inserta recursivamente; devuelve el hermano si el nodo se partio.
fn insert_recursive(node: &mut Node, point: (f64, f64), max_entries: usize) -> Option<Node> {
    match node {
        Node::Leaf { mbr, points } => {
            points.push(point);
            // El MBR de la hoja debe crecer con el punto nuevo (si no, el padre
            // poda subarboles que si contienen el punto).
            *mbr = mbr.union(Rect::point(point.0, point.1));
            if points.len() > max_entries {
                Some(split_leaf(node, max_entries))
            } else {
                None
            }
        }
        Node::Branch { mbr, children } => {
            let target = choose_child(children, point);
            let split = insert_recursive(&mut children[target], point, max_entries);
            if let Some(sibling) = split {
                children.push(sibling);
            }
            let mut bounds = children[0].mbr();
            for child in &children[1..] {
                bounds = bounds.union(child.mbr());
            }
            *mbr = bounds;
            if children.len() > max_entries {
                Some(split_branch(node, max_entries))
            } else {
                None
            }
        }
    }
}

/// Elige el hijo con menor incremento de area (desempate: menor area).
fn choose_child(children: &[Node], point: (f64, f64)) -> usize {
    let point_rect = Rect::point(point.0, point.1);
    let mut best = 0usize;
    let mut best_enlargement = f64::INFINITY;
    let mut best_area = f64::INFINITY;
    for (index, child) in children.iter().enumerate() {
        let mbr = child.mbr();
        let enlargement = mbr.enlargement(&point_rect);
        let area = mbr.area();
        if enlargement < best_enlargement || (enlargement == best_enlargement && area < best_area) {
            best = index;
            best_enlargement = enlargement;
            best_area = area;
        }
    }
    best
}

/// Parte una hoja por la mediana del eje de mayor dispersion.
fn split_leaf(node: &mut Node, _max_entries: usize) -> Node {
    if let Node::Leaf { points, .. } = node {
        let (min_x, max_x, min_y, max_y) = bounds_of(points);
        let sort_by_x = (max_x - min_x) >= (max_y - min_y);
        points.sort_by(|left, right| {
            let key = if sort_by_x {
                left.0.partial_cmp(&right.0)
            } else {
                left.1.partial_cmp(&right.1)
            };
            key.unwrap_or(std::cmp::Ordering::Equal)
        });
        let right_points = points.split_off(points.len() / 2);
        let right_mbr = mbr_of(&right_points);
        node.recompute();
        Node::Leaf {
            mbr: right_mbr,
            points: right_points,
        }
    } else {
        unreachable!("split_leaf sobre un nodo interno")
    }
}

/// Parte un nodo interno por la mediana del eje de mayor dispersion.
fn split_branch(node: &mut Node, _max_entries: usize) -> Node {
    if let Node::Branch { children, .. } = node {
        let centers: Vec<(f64, f64)> = children
            .iter()
            .map(|child| (child.mbr().center_x(), child.mbr().center_y()))
            .collect();
        let (min_x, max_x, min_y, max_y) = bounds_of(&centers);
        let sort_by_x = (max_x - min_x) >= (max_y - min_y);
        children.sort_by(|left, right| {
            let (left_x, left_y) = (left.mbr().center_x(), left.mbr().center_y());
            let (right_x, right_y) = (right.mbr().center_x(), right.mbr().center_y());
            let key = if sort_by_x {
                left_x.partial_cmp(&right_x)
            } else {
                left_y.partial_cmp(&right_y)
            };
            key.unwrap_or(std::cmp::Ordering::Equal)
        });
        let right_children = children.split_off(children.len() / 2);
        let right_mbr = right_children
            .first()
            .map(|child| child.mbr())
            .map(|first| {
                right_children[1..]
                    .iter()
                    .fold(first, |bounds, child| bounds.union(child.mbr()))
            })
            .unwrap_or(Rect::point(0.0, 0.0));
        node.recompute();
        Node::Branch {
            mbr: right_mbr,
            children: right_children,
        }
    } else {
        unreachable!("split_branch sobre una hoja")
    }
}

/// Consulta recursiva con poda por MBR.
fn query_recursive(node: &Node, window: &Rect, out: &mut Vec<(f64, f64)>) {
    if !node.mbr().intersects(window) {
        return;
    }
    match node {
        Node::Leaf { points, .. } => {
            for &(x, y) in points {
                if window.contains(x, y) {
                    out.push((x, y));
                }
            }
        }
        Node::Branch { children, .. } => {
            for child in children {
                query_recursive(child, window, out);
            }
        }
    }
}

/// Limites `(min_x, max_x, min_y, max_y)` de una lista de puntos.
fn bounds_of(points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), &(x, y)| {
            (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
        },
    )
}

/// MBR de una lista de puntos.
fn mbr_of(points: &[(f64, f64)]) -> Rect {
    let (min_x, max_x, min_y, max_y) = bounds_of(points);
    Rect {
        min_x,
        min_y,
        max_x,
        max_y,
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
        let mut hits = tree.range_query(0.0, 0.0, 5.0, 5.0);
        hits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        assert_eq!(hits, vec![(1.0, 2.0), (3.0, 4.0)]);
        assert!(tree.range_query(100.0, 100.0, 200.0, 200.0).is_empty());
    }

    /// Con muchos puntos, el rango coincide con la fuerza bruta y el arbol
    /// tiene altura > 1 (hubo splits).
    #[test]
    fn range_query_matches_bruteforce() {
        let mut tree = RTreeSplit::with_max_entries(4);
        let mut points = Vec::new();
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64 * 100.0
        };
        for _ in 0..1_000 {
            let (x, y) = (next(), next());
            points.push((x, y));
            tree.insert(x, y);
        }
        assert_eq!(tree.len(), 1_000);
        assert!(tree.height() > 1, "debe haber splits");
        let (min_x, min_y, max_x, max_y) = tree.mbr().expect("mbr");
        assert!(min_x <= max_x && min_y <= max_y);

        for _ in 0..50 {
            let x0 = next();
            let y0 = next();
            let x1 = x0 + 5.0 + next() % 20.0;
            let y1 = y0 + 5.0 + next() % 20.0;
            let mut expected: Vec<(f64, f64)> = points
                .iter()
                .copied()
                .filter(|&(x, y)| x >= x0 && x <= x1 && y >= y0 && y <= y1)
                .collect();
            let mut got = tree.range_query(x0, y0, x1, y1);
            expected.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            got.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            assert_eq!(got, expected);
        }
    }
}
