//! # ruscadb-graph
//!
//! Almacén de grafo de RuscaDB: adyacencia **CSR** (Compressed Sparse Row) para
//! salientes y entrantes, más traversal BFS acotado por profundidad y número de
//! nodos (protección de RAM) y los algoritmos [`bfs`], [`connected_components`]
//! y [`pagerank`].
//!
//! Especificación: `specs/graph_store.md` (SPEC-0007) y
//! `specs/graph_algorithms.md` (SPEC-0041).
//! Diseño: `docs/RuscaDB-roadmap.md` §5.4 (Fase F3b).

#![forbid(unsafe_code)]

mod algorithms;
mod csr;
mod traversal;

pub use algorithms::{bfs, connected_components, pagerank};
pub use csr::CsrGraph;

/// Identificador de nodo dentro del grafo.
pub type NodeId = u64;

/// Dirección de la adyacencia consultada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Aristas salientes (`from -> to`).
    Out,
    /// Aristas entrantes (`from -> to` vistas desde el destino `to`).
    In,
    /// Unión ordenada de [`Direction::Out`] y [`Direction::In`].
    Both,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::{BTreeMap, HashSet, VecDeque};

    /// Construye un grafo ya compilado a partir de una lista de aristas.
    ///
    /// Args:
    ///     edges: Aristas `(from, to)` a insertar.
    ///
    /// Returns:
    ///     El grafo con el CSR recomputado.
    fn graph_from(edges: &[(NodeId, NodeId)]) -> CsrGraph {
        let mut graph = CsrGraph::new();
        for &(from, to) in edges {
            graph.add_edge(from, to);
        }
        graph.build();
        graph
    }

    /// Oráculo: distancias BFS mínimas desde `start` (sin cota de profundidad).
    ///
    /// Args:
    ///     graph: Grafo compilado a consultar.
    ///     start: Nodo de origen.
    ///     direction: Dirección de recorrido.
    ///
    /// Returns:
    ///     Mapa de nodo alcanzable a su distancia mínima (0 para `start`).
    fn bfs_distances(
        graph: &CsrGraph,
        start: NodeId,
        direction: Direction,
    ) -> BTreeMap<NodeId, u16> {
        let mut distances = BTreeMap::new();
        if !graph.contains(start) {
            return distances;
        }
        let mut queue = VecDeque::new();
        distances.insert(start, 0);
        queue.push_back(start);
        while let Some(node) = queue.pop_front() {
            let depth = *distances.get(&node).expect("distancia registrada");
            if depth == u16::MAX {
                continue;
            }
            for neighbor in graph.neighbors(node, direction) {
                if let std::collections::btree_map::Entry::Vacant(entry) = distances.entry(neighbor)
                {
                    entry.insert(depth + 1);
                    queue.push_back(neighbor);
                }
            }
        }
        distances
    }

    /// Estrategia de aristas sobre un espacio pequeño de nodos (0..8).
    fn edge_strategy() -> impl Strategy<Value = Vec<(NodeId, NodeId)>> {
        prop::collection::vec((0u64..8, 0u64..8), 0..40)
    }

    /// AC-0007-01 — aristas 1->2 y 1->3 dan vecinos salientes `{2,3}`.
    #[test]
    // @spec AC-0007-01
    fn test_ac_0007_01_add_edge_and_neighbors() {
        let graph = graph_from(&[(1, 2), (1, 3)]);
        assert_eq!(graph.neighbors(1, Direction::Out), vec![2, 3]);
        assert_eq!(graph.neighbors(2, Direction::In), vec![1]);
        assert_eq!(graph.neighbors(3, Direction::In), vec![1]);
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
    }

    /// AC-0007-02 — cadena 1->2->3->4; BFS con profundidad 1 devuelve `{1,2}`.
    #[test]
    // @spec AC-0007-02
    fn test_ac_0007_02_bfs_respects_depth() {
        let graph = graph_from(&[(1, 2), (2, 3), (3, 4)]);
        let depth_one = graph.traverse(1, Direction::Out, 1, usize::MAX);
        assert_eq!(depth_one, vec![1, 2]);
        assert!(!depth_one.contains(&3));
        assert!(!depth_one.contains(&4));
        assert_eq!(graph.traverse(1, Direction::Out, 0, usize::MAX), vec![1]);
        assert_eq!(
            graph.traverse(1, Direction::Out, 2, usize::MAX),
            vec![1, 2, 3]
        );
        assert_eq!(
            graph.traverse(1, Direction::Out, 3, usize::MAX),
            vec![1, 2, 3, 4]
        );
    }

    /// AC-0007-03 — el traversal nunca supera `max_nodes`.
    #[test]
    // @spec AC-0007-03
    fn test_ac_0007_03_max_nodes_limit() {
        let graph = graph_from(&[(1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7), (7, 8)]);
        let limited = graph.traverse(1, Direction::Out, u16::MAX, 3);
        assert!(limited.len() <= 3);
        assert_eq!(limited, vec![1, 2, 3]);
        assert!(graph.traverse(1, Direction::Out, u16::MAX, 0).is_empty());
    }

    /// AC-0007-04 — nodo desconocido: vecinos y traversal vacíos, sin panic.
    #[test]
    // @spec AC-0007-04
    fn test_ac_0007_04_unknown_node_is_empty() {
        let graph = graph_from(&[(1, 2), (2, 3)]);
        assert!(graph.neighbors(999, Direction::Out).is_empty());
        assert!(graph.neighbors(999, Direction::In).is_empty());
        assert!(graph.neighbors(999, Direction::Both).is_empty());
        assert!(graph.traverse(999, Direction::Out, 5, 100).is_empty());
        assert!(
            graph
                .traverse(999, Direction::Both, u16::MAX, usize::MAX)
                .is_empty()
        );
    }

    /// Un grafo nuevo está vacío y no entra en panic al consultarse.
    #[test]
    fn test_new_graph_is_empty() {
        let graph = CsrGraph::new();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert!(graph.neighbors(0, Direction::Out).is_empty());
        assert!(graph.traverse(0, Direction::Out, 10, 10).is_empty());
    }

    /// `Default` es equivalente a `new`.
    #[test]
    fn test_default_matches_new() {
        let graph = CsrGraph::default();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    /// Las aristas duplicadas no generan vecinos ni conteos duplicados.
    #[test]
    fn test_duplicate_edges_are_deduplicated() {
        let graph = graph_from(&[(1, 2), (1, 2), (1, 2)]);
        assert_eq!(graph.neighbors(1, Direction::Out), vec![2]);
        assert_eq!(graph.edge_count(), 1);
        assert_eq!(graph.node_count(), 2);
    }

    /// `Direction::Both` es la unión ordenada de salientes y entrantes.
    #[test]
    fn test_both_is_union_of_out_and_in() {
        let graph = graph_from(&[(1, 2), (3, 1)]);
        assert_eq!(graph.neighbors(1, Direction::Out), vec![2]);
        assert_eq!(graph.neighbors(1, Direction::In), vec![3]);
        assert_eq!(graph.neighbors(1, Direction::Both), vec![2, 3]);
    }

    /// Un nodo sin aristas en una dirección concreta devuelve vacío.
    #[test]
    fn test_missing_direction_is_empty() {
        let graph = graph_from(&[(1, 2)]);
        assert!(graph.neighbors(2, Direction::Out).is_empty());
        assert!(graph.neighbors(1, Direction::In).is_empty());
    }

    /// Un auto-lazo es visible en ambas direcciones sin duplicarse en `Both`.
    #[test]
    fn test_self_loop() {
        let graph = graph_from(&[(1, 1)]);
        assert_eq!(graph.neighbors(1, Direction::Out), vec![1]);
        assert_eq!(graph.neighbors(1, Direction::In), vec![1]);
        assert_eq!(graph.neighbors(1, Direction::Both), vec![1]);
        assert_eq!(graph.edge_count(), 1);
        assert_eq!(graph.node_count(), 1);
    }

    /// BFS en sentido entrante recorre las aristas al revés.
    #[test]
    fn test_traverse_incoming() {
        let graph = graph_from(&[(1, 2), (2, 3), (3, 4)]);
        assert_eq!(
            graph.traverse(4, Direction::In, 2, usize::MAX),
            vec![4, 3, 2]
        );
        assert_eq!(
            graph.traverse(1, Direction::In, u16::MAX, usize::MAX),
            vec![1]
        );
    }

    /// `max_nodes == 0` devuelve vacío incluso para un start válido.
    #[test]
    fn test_max_nodes_zero_is_empty() {
        let graph = graph_from(&[(1, 2)]);
        assert!(graph.traverse(1, Direction::Out, u16::MAX, 0).is_empty());
    }

    /// `build` es idempotente sobre el mismo conjunto de aristas.
    #[test]
    fn test_build_is_idempotent() {
        let mut graph = graph_from(&[(1, 2), (2, 3)]);
        let before = graph.traverse(1, Direction::Out, u16::MAX, usize::MAX);
        graph.build();
        let after = graph.traverse(1, Direction::Out, u16::MAX, usize::MAX);
        assert_eq!(before, after);
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
    }

    /// `add_edge` tras un `build` se refleja tras recomputar el CSR.
    #[test]
    fn test_add_edge_incremental_rebuild() {
        let mut graph = graph_from(&[(1, 2)]);
        graph.add_edge(2, 3);
        graph.build();
        assert_eq!(
            graph.traverse(1, Direction::Out, u16::MAX, usize::MAX),
            vec![1, 2, 3]
        );
    }

    /// El traversal no incluye nodos de componentes desconectadas.
    #[test]
    fn test_traverse_only_reachable_nodes() {
        let graph = graph_from(&[(1, 2), (3, 4)]);
        assert_eq!(
            graph.traverse(1, Direction::Out, u16::MAX, usize::MAX),
            vec![1, 2]
        );
    }

    /// `traverse` respeta `max_depth` y `max_nodes` de forma simultánea.
    #[test]
    fn test_traverse_limits_combined() {
        let graph = graph_from(&[(1, 2), (2, 3), (3, 4), (4, 5)]);
        assert_eq!(graph.traverse(1, Direction::Out, 2, 2), vec![1, 2]);
        assert_eq!(graph.traverse(1, Direction::Out, 1, 100), vec![1, 2]);
        assert_eq!(
            graph.traverse(1, Direction::Out, 0, 0),
            Vec::<NodeId>::new()
        );
    }

    proptest! {
        /// Los vecinos siempre están ordenados de forma ascendente y sin duplicados.
        #[test]
        fn prop_neighbors_sorted_unique(edges in edge_strategy(), node in 0u64..10) {
            let graph = graph_from(&edges);
            for direction in [Direction::Out, Direction::In, Direction::Both] {
                let neighbors = graph.neighbors(node, direction);
                let mut expected = neighbors.clone();
                expected.sort_unstable();
                expected.dedup();
                prop_assert_eq!(neighbors, expected);
            }
        }

        /// `traverse` respeta `max_nodes`/`max_depth` y solo devuelve alcanzables.
        #[test]
        fn prop_traverse_bounded(
            edges in edge_strategy(),
            start in 0u64..10,
            max_nodes in 0usize..12,
            max_depth in 0u16..6,
        ) {
            let graph = graph_from(&edges);
            let distances = bfs_distances(&graph, start, Direction::Out);
            let result = graph.traverse(start, Direction::Out, max_depth, max_nodes);
            prop_assert!(result.len() <= max_nodes);
            let unique: HashSet<NodeId> = result.iter().copied().collect();
            prop_assert_eq!(unique.len(), result.len());
            for node in &result {
                let distance = distances.get(node).copied();
                prop_assert!(distance.is_some(), "nodo {} no es alcanzable", node);
                prop_assert!(distance.unwrap() <= max_depth);
            }
            if max_nodes > 0 && graph.contains(start) {
                prop_assert_eq!(result.first().copied(), Some(start));
            }
        }

        /// El traversal acotado es subconjunto del traversal sin cota.
        #[test]
        fn prop_traverse_subset_of_unbounded(
            edges in edge_strategy(),
            start in 0u64..10,
            max_nodes in 0usize..12,
            max_depth in 0u16..6,
        ) {
            let graph = graph_from(&edges);
            let full: HashSet<NodeId> = graph
                .traverse(start, Direction::Both, u16::MAX, usize::MAX)
                .into_iter()
                .collect();
            let bounded = graph.traverse(start, Direction::Both, max_depth, max_nodes);
            for node in &bounded {
                prop_assert!(full.contains(node));
            }
        }
    }
}
