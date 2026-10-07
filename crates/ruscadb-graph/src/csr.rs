//! Almacén de adyacencia **CSR** (Compressed Sparse Row).
//!
//! Acumula aristas y, en [`CsrGraph::build`], materializa dos estructuras CSR
//! compactas: una para aristas salientes (`Out`) y otra para entrantes (`In`).

use std::cmp::Ordering;

use crate::{Direction, NodeId};

/// Grafo dirigido en formato CSR con adyacencia saliente y entrante.
///
/// # Invariantes
/// - `out_neighbors`/`in_neighbors` están ordenados de forma ascendente y sin
///   duplicados dentro de cada nodo.
/// - `contains` es la única vía de validación: los nodos desconocidos devuelven
///   vecinos vacíos, sin panic.
/// - `node_count`/`edge_count` reflejan el último [`CsrGraph::build`].
pub struct CsrGraph {
    edges: Vec<(NodeId, NodeId)>,
    nodes: Vec<NodeId>,
    out_offsets: Vec<usize>,
    out_neighbors: Vec<NodeId>,
    in_offsets: Vec<usize>,
    in_neighbors: Vec<NodeId>,
    node_count: usize,
    edge_count: usize,
}

impl CsrGraph {
    /// Crea un grafo vacío.
    ///
    /// Returns:
    ///     Un grafo sin nodos ni aristas.
    pub fn new() -> Self {
        Self {
            edges: Vec::new(),
            nodes: Vec::new(),
            out_offsets: vec![0],
            out_neighbors: Vec::new(),
            in_offsets: vec![0],
            in_neighbors: Vec::new(),
            node_count: 0,
            edge_count: 0,
        }
    }

    /// Añade una arista dirigida `from -> to`.
    ///
    /// El CSR no se actualiza hasta llamar a [`CsrGraph::build`].
    ///
    /// Args:
    ///     from: Nodo de origen.
    ///     to: Nodo de destino.
    pub fn add_edge(&mut self, from: NodeId, to: NodeId) {
        self.edges.push((from, to));
    }

    /// Recomputa el CSR (offsets + vecinos) para salientes y entrantes.
    ///
    /// Deduplica aristas y vecinos, y recalcula los conteos.
    pub fn build(&mut self) {
        let mut nodes = Vec::with_capacity(self.edges.len().saturating_mul(2));
        for &(from, to) in &self.edges {
            nodes.push(from);
            nodes.push(to);
        }
        nodes.sort_unstable();
        nodes.dedup();
        let count = nodes.len();

        let mut out_adjacency = vec![Vec::new(); count];
        let mut in_adjacency = vec![Vec::new(); count];
        for &(from, to) in &self.edges {
            if let (Ok(from_index), Ok(to_index)) =
                (nodes.binary_search(&from), nodes.binary_search(&to))
            {
                out_adjacency[from_index].push(to);
                in_adjacency[to_index].push(from);
            }
        }

        let (out_offsets, out_neighbors) = flatten_adjacency(&mut out_adjacency);
        let (in_offsets, in_neighbors) = flatten_adjacency(&mut in_adjacency);

        self.node_count = count;
        self.edge_count = out_neighbors.len();
        self.nodes = nodes;
        self.out_offsets = out_offsets;
        self.out_neighbors = out_neighbors;
        self.in_offsets = in_offsets;
        self.in_neighbors = in_neighbors;
    }

    /// Devuelve los vecinos directos de `node` en la `direction` indicada.
    ///
    /// Args:
    ///     node: Nodo consultado.
    ///     direction: Dirección de adyacencia.
    ///
    /// Returns:
    ///     Vecinos ordenados de forma ascendente y sin duplicados; vacío si el
    ///     nodo es desconocido.
    pub fn neighbors(&self, node: NodeId, direction: Direction) -> Vec<NodeId> {
        let Some(index) = self.index_of(node) else {
            return Vec::new();
        };
        match direction {
            Direction::Out => self.out_slice(index).to_vec(),
            Direction::In => self.in_slice(index).to_vec(),
            Direction::Both => merge_unique(self.out_slice(index), self.in_slice(index)),
        }
    }

    /// Número de nodos distintos del último [`CsrGraph::build`].
    ///
    /// Returns:
    ///     Cantidad de nodos del grafo compilado.
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Número de aristas únicas del último [`CsrGraph::build`].
    ///
    /// Returns:
    ///     Cantidad de aristas dirigidas únicas.
    pub fn edge_count(&self) -> usize {
        self.edge_count
    }

    /// Indica si `node` pertenece al grafo compilado.
    ///
    /// Args:
    ///     node: Nodo a comprobar.
    ///
    /// Returns:
    ///     `true` si el nodo es conocido.
    pub(crate) fn contains(&self, node: NodeId) -> bool {
        self.index_of(node).is_some()
    }

    /// Posición densa de `node` en el CSR, o `None` si es desconocido.
    ///
    /// Args:
    ///     node: Nodo consultado.
    ///
    /// Returns:
    ///     El índice `0..node_count`, o `None`.
    fn index_of(&self, node: NodeId) -> Option<usize> {
        self.nodes.binary_search(&node).ok()
    }

    /// Rebanada de vecinos salientes del nodo en la posición `index`.
    ///
    /// Args:
    ///     index: Índice denso válido del nodo.
    ///
    /// Returns:
    ///     Vecinos salientes ordenados y sin duplicados.
    fn out_slice(&self, index: usize) -> &[NodeId] {
        slice_for(&self.out_offsets, &self.out_neighbors, index)
    }

    /// Rebanada de vecinos entrantes del nodo en la posición `index`.
    ///
    /// Args:
    ///     index: Índice denso válido del nodo.
    ///
    /// Returns:
    ///     Vecinos entrantes ordenados y sin duplicados.
    fn in_slice(&self, index: usize) -> &[NodeId] {
        slice_for(&self.in_offsets, &self.in_neighbors, index)
    }
}

impl Default for CsrGraph {
    /// Crea un grafo vacío equivalente a [`CsrGraph::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Extrae la rebanada `[offsets[index], offsets[index + 1])`.
///
/// Args:
///     offsets: Vector CSR de `n + 1` offsets.
///     neighbors: Vecinos concatenados.
///     index: Índice del nodo.
///
/// Returns:
///     La rebanada de vecinos o vacía si el índice está fuera de rango.
fn slice_for<'a>(offsets: &[usize], neighbors: &'a [NodeId], index: usize) -> &'a [NodeId] {
    let start = offsets.get(index).copied().unwrap_or(0);
    let end = offsets.get(index.wrapping_add(1)).copied().unwrap_or(start);
    neighbors.get(start..end).unwrap_or(&[])
}

/// Ordena y deduplica cada lista y la aplana en `(offsets, neighbors)`.
///
/// Args:
///     adjacency: Listas de adyacencia por nodo.
///
/// Returns:
///     El par CSR `(offsets, neighbors)`.
fn flatten_adjacency(adjacency: &mut [Vec<NodeId>]) -> (Vec<usize>, Vec<NodeId>) {
    let mut offsets = Vec::with_capacity(adjacency.len() + 1);
    let mut neighbors = Vec::new();
    offsets.push(0);
    for list in adjacency.iter_mut() {
        list.sort_unstable();
        list.dedup();
        neighbors.extend_from_slice(list);
        offsets.push(neighbors.len());
    }
    (offsets, neighbors)
}

/// Fusiona dos rebanadas ordenadas y sin duplicados en una sola, sin duplicados.
///
/// Args:
///     left: Primera lista ordenada.
///     right: Segunda lista ordenada.
///
/// Returns:
///     Unión ordenada ascendente y sin duplicados.
fn merge_unique(left: &[NodeId], right: &[NodeId]) -> Vec<NodeId> {
    let mut merged = Vec::with_capacity(left.len() + right.len());
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            Ordering::Less => {
                merged.push(left[left_index]);
                left_index += 1;
            }
            Ordering::Greater => {
                merged.push(right[right_index]);
                right_index += 1;
            }
            Ordering::Equal => {
                merged.push(left[left_index]);
                left_index += 1;
                right_index += 1;
            }
        }
    }
    merged.extend_from_slice(&left[left_index..]);
    merged.extend_from_slice(&right[right_index..]);
    merged
}
