//! Benchmark de algoritmos de grafo contra oráculo congelado (SPEC-0198).

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use ruscadb_graph::{CsrGraph, bfs, connected_components, pagerank};

/// AC-0198-02 — BFS, componentes y PageRank sobre un grafo determinista.
#[test] // @spec AC-0198-02
fn test_ac_0198_02_graph_benchmark() {
    let mut graph = CsrGraph::new();
    for (from, to) in [(0u64, 1u64), (1, 2), (2, 0), (3, 4)] {
        graph.add_edge(from, to);
    }
    graph.build();

    assert_eq!(bfs(&graph, 0), vec![0, 1, 2]);
    let mut components = connected_components(&graph);
    components.sort();
    assert_eq!(components, vec![vec![0, 1, 2], vec![3, 4]]);

    let ranks = pagerank(&graph, 0.85, 100, 1e-9).expect("pagerank");
    let total: f64 = ranks.iter().sum();
    assert!((total - 1.0).abs() < 1e-6, "suma de PageRank={total}");
}
