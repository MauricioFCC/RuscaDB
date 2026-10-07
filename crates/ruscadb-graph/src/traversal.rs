//! Traversal BFS acotado sobre el CSR de [`CsrGraph`].

use std::collections::{HashSet, VecDeque};

use crate::{CsrGraph, Direction, NodeId};

impl CsrGraph {
    /// Recorre en anchura (BFS) desde `start`, acotado por profundidad y nodos.
    ///
    /// Incluye `start` con profundidad 0. Nunca devuelve más de `max_nodes`
    /// nodos ni visita nodos a profundidad mayor que `max_depth`. Un `start`
    /// desconocido o `max_nodes == 0` devuelve vacío (sin panic).
    ///
    /// Args:
    ///     start: Nodo de origen.
    ///     direction: Dirección de recorrido de las aristas.
    ///     max_depth: Profundidad máxima (0 solo devuelve `start`).
    ///     max_nodes: Número máximo de nodos en el resultado.
    ///
    /// Returns:
    ///     Nodos alcanzables en orden BFS, sin duplicados.
    pub fn traverse(
        &self,
        start: NodeId,
        direction: Direction,
        max_depth: u16,
        max_nodes: usize,
    ) -> Vec<NodeId> {
        if max_nodes == 0 || !self.contains(start) {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start);
        queue.push_back((start, 0u16));
        while let Some((node, depth)) = queue.pop_front() {
            result.push(node);
            if result.len() >= max_nodes {
                break;
            }
            if depth >= max_depth {
                continue;
            }
            for neighbor in self.neighbors(node, direction) {
                if visited.insert(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
        result
    }
}
