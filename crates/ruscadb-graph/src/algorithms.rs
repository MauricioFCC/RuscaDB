//! Algoritmos de grafo sobre el CSR de [`CsrGraph`]: BFS, componentes
//! débilmente conexas y PageRank.
//!
//! # Frontera (citas)
//!
//! - **BFS**: Cormen, Leiserson, Rivest & Stein, *Introduction to Algorithms*
//!   (CLRS), §22.2 «Breadth-first search» — cola FIFO + marcado de visitados,
//!   complejidad `O(V + E)`.
//! - **PageRank**: Brin & Page (1998), «The Anatomy of a Large-Scale
//!   Hypertextual Web Search Engine», *Computer Networks* 30(1-7), 107-117 —
//!   método de la potencia con factor de amortiguación (*damping*), criterio de
//!   convergencia por norma L1 y redistribución uniforme de la masa de los
//!   nodos colgantes (*dangling*).
//! - **Componentes débilmente conexas**: BFS sobre la adyacencia no dirigida
//!   (unión de aristas salientes y entrantes). Se elige BFS frente a
//!   *union-find* por simplicidad, reutilización del CSR y la misma cota
//!   `O(V + E)`; *union-find* sería preferible si se necesitase procesar
//!   aristas en streaming sin materializar el CSR.
//!
//! # Complejidades
//!
//! - [`bfs`]: `O(V + E)`.
//! - [`connected_components`]: `O(V + E)`.
//! - [`pagerank`]: `O(max_iters · (V + E))`.

use std::collections::{HashSet, VecDeque};

use ruscadb_core::RuscaError;

use crate::{CsrGraph, Direction, NodeId};

/// Recorre el grafo en anchura (BFS) desde `start` siguiendo aristas salientes.
///
/// Cita CLRS §22.2. El orden es determinista porque
/// [`CsrGraph::neighbors`] devuelve vecinos ascendentes y sin duplicados, de
/// modo que cada nivel se expande en orden creciente de identificador. Un
/// `start` desconocido devuelve vacío (sin panic).
///
/// Complejidad: `O(V + E)`.
///
/// Args:
///     graph: Grafo compilado a recorrer.
///     start: Nodo de origen.
///
/// Returns:
///     Nodos en orden de visita BFS, cada uno exactamente una vez.
pub fn bfs(graph: &CsrGraph, start: NodeId) -> Vec<NodeId> {
    if !graph.contains(start) {
        return Vec::new();
    }
    let mut order = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(start);
    queue.push_back(start);
    while let Some(node) = queue.pop_front() {
        order.push(node);
        for neighbor in graph.neighbors(node, Direction::Out) {
            if visited.insert(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }
    order
}

/// Particiona el grafo en componentes **débilmente conexas** (ignora la
/// dirección de las aristas).
///
/// Cada nodo aparece en exactamente una componente. El orden es determinista:
/// los nodos se visitan en orden ascendente y cada componente se devuelve
/// ordenada de forma ascendente, por lo que la lista de componentes queda
/// ordenada por su nodo mínimo.
///
/// Complejidad: `O(V + E)`.
///
/// Args:
///     graph: Grafo compilado a particionar.
///
/// Returns:
///     Componentes débilmente conexas; vacío para un grafo sin nodos.
pub fn connected_components(graph: &CsrGraph) -> Vec<Vec<NodeId>> {
    let mut visited = HashSet::new();
    let mut components = Vec::new();
    for &node in graph.node_ids() {
        if visited.contains(&node) {
            continue;
        }
        components.push(collect_component(graph, node, &mut visited));
    }
    components
}

/// Expande la componente débilmente conexa que contiene a `start`.
///
/// Args:
///     graph: Grafo compilado.
///     start: Nodo semilla (aún no visitado).
///     visited: Conjunto de nodos ya asignados a una componente.
///
/// Returns:
///     Nodos de la componente, ordenados de forma ascendente.
fn collect_component(
    graph: &CsrGraph,
    start: NodeId,
    visited: &mut HashSet<NodeId>,
) -> Vec<NodeId> {
    let mut component = Vec::new();
    let mut queue = VecDeque::new();
    visited.insert(start);
    queue.push_back(start);
    while let Some(node) = queue.pop_front() {
        component.push(node);
        for neighbor in graph.neighbors(node, Direction::Both) {
            if visited.insert(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }
    component.sort_unstable();
    component
}

/// Calcula PageRank por el método de la potencia (Brin & Page 1998).
///
/// Itera hasta que la norma L1 entre dos vectores consecutivos cae por debajo
/// de `tol` o se alcanza `max_iters`. Los nodos colgantes (sin aristas de
/// salida) redistribuyen su masa **uniformemente** entre todos los nodos, tal
/// como describe el artículo original. El resultado se normaliza para que su
/// suma sea 1 y no contiene NaN.
///
/// Complejidad: `O(max_iters · (V + E))`.
///
/// Args:
///     graph: Grafo compilado sobre el que puntuar los nodos.
///     damping: Factor de amortiguación `d` (debe cumplir `0 < d < 1`).
///     max_iters: Cota superior del número de iteraciones (debe ser `> 0`).
///     tol: Tolerancia de convergencia L1 (debe ser `> 0`).
///
/// Returns:
///     Vector de scores alineado con el orden ascendente de nodos; vacío si el
///     grafo no tiene nodos.
///
/// Raises:
///     RuscaError::InvalidConfig: Si `damping` no está en `(0, 1)`, si
///         `max_iters == 0` o si `tol` no es `> 0` (incluye NaN).
pub fn pagerank(
    graph: &CsrGraph,
    damping: f64,
    max_iters: usize,
    tol: f64,
) -> Result<Vec<f64>, RuscaError> {
    validate_pagerank(damping, max_iters, tol)?;
    let nodes = graph.node_ids();
    let count = nodes.len();
    if count == 0 {
        return Ok(Vec::new());
    }
    let out_degree = out_degrees(graph, nodes);
    let context = RankContext {
        graph,
        nodes,
        out_degree: &out_degree,
        damping,
        teleport: (1.0 - damping) / count as f64,
    };
    let mut ranks = vec![1.0 / count as f64; count];
    for _ in 0..max_iters {
        let next = context.step(&ranks);
        let delta: f64 = ranks
            .iter()
            .zip(&next)
            .map(|(previous, current)| (current - previous).abs())
            .sum();
        ranks = next;
        if delta <= tol {
            break;
        }
    }
    normalize(&mut ranks);
    Ok(ranks)
}

/// Valida los parámetros de [`pagerank`].
///
/// Args:
///     damping: Factor de amortiguación.
///     max_iters: Cota de iteraciones.
///     tol: Tolerancia de convergencia.
///
/// Returns:
///     `Ok(())` si los parámetros son admisibles.
///
/// Raises:
///     RuscaError::InvalidConfig: Ante cualquier parámetro fuera de rango.
fn validate_pagerank(damping: f64, max_iters: usize, tol: f64) -> Result<(), RuscaError> {
    if !damping.is_finite() || damping <= 0.0 || damping >= 1.0 {
        return Err(RuscaError::InvalidConfig(format!(
            "damping debe estar en (0, 1) (recibido {damping}; revisa pagerank())"
        )));
    }
    if max_iters == 0 {
        return Err(RuscaError::InvalidConfig(
            "max_iters debe ser > 0 (recibe 0; revisa pagerank())".to_string(),
        ));
    }
    if !tol.is_finite() || tol <= 0.0 {
        return Err(RuscaError::InvalidConfig(format!(
            "tol debe ser > 0 (recibida {tol}; revisa pagerank())"
        )));
    }
    Ok(())
}

/// Grados de salida por nodo (posición paralela a `nodes`).
///
/// Args:
///     graph: Grafo compilado.
///     nodes: Nodos en orden ascendente.
///
/// Returns:
///     Grado de salida de cada nodo, en el mismo orden que `nodes`.
fn out_degrees(graph: &CsrGraph, nodes: &[NodeId]) -> Vec<usize> {
    nodes
        .iter()
        .map(|&node| graph.neighbors(node, Direction::Out).len())
        .collect()
}

/// Contexto inmutable reutilizable por cada paso de la iteración de potencia.
struct RankContext<'a> {
    /// Grafo bajo análisis.
    graph: &'a CsrGraph,
    /// Nodos en orden ascendente (índice denso del vector PageRank).
    nodes: &'a [NodeId],
    /// Grado de salida por nodo (posición paralela a `nodes`).
    out_degree: &'a [usize],
    /// Factor de amortiguación `d`.
    damping: f64,
    /// Cuota de teletransporte `(1 - d) / V` por nodo.
    teleport: f64,
}

impl RankContext<'_> {
    /// Aplica una iteración del método de la potencia.
    ///
    /// Args:
    ///     ranks: Vector PageRank de la iteración previa.
    ///
    /// Returns:
    ///     Vector PageRank de la siguiente iteración (sin normalizar).
    fn step(&self, ranks: &[f64]) -> Vec<f64> {
        let count = self.nodes.len();
        let dangling: f64 = ranks
            .iter()
            .zip(self.out_degree)
            .filter(|(_, degree)| **degree == 0)
            .map(|(rank, _)| rank)
            .sum();
        let dangling_share = self.damping * dangling / count as f64;
        let mut next = vec![self.teleport + dangling_share; count];
        for (index, &node) in self.nodes.iter().enumerate() {
            let mut incoming = 0.0;
            for predecessor in self.graph.neighbors(node, Direction::In) {
                if let Ok(pred_index) = self.nodes.binary_search(&predecessor) {
                    incoming += ranks[pred_index] / self.out_degree[pred_index] as f64;
                }
            }
            next[index] += self.damping * incoming;
        }
        next
    }
}

/// Normaliza un vector para que su suma sea 1 (si es positiva y finita).
///
/// Args:
///     ranks: Vector PageRank a normalizar en sitio.
fn normalize(ranks: &mut [f64]) {
    let total: f64 = ranks.iter().sum();
    if total > 0.0 && total.is_finite() {
        for rank in ranks.iter_mut() {
            *rank /= total;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::HashSet;

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

    /// Estrategia de aristas sobre un espacio pequeño de nodos (0..8).
    fn edge_strategy() -> impl Strategy<Value = Vec<(NodeId, NodeId)>> {
        prop::collection::vec((0u64..8, 0u64..8), 0..40)
    }

    /// Suma total de un vector de scores.
    ///
    /// Args:
    ///     scores: Vector de scores PageRank.
    ///
    /// Returns:
    ///     La suma de todos los elementos.
    fn total(scores: &[f64]) -> f64 {
        scores.iter().sum()
    }

    /// AC-0041-01 — BFS respeta niveles y no repite nodos.
    #[test]
    fn test_ac_0041_01_bfs_order() {
        let graph = graph_from(&[(1, 2), (1, 3), (2, 4), (3, 4), (4, 5), (5, 1)]);
        let order = bfs(&graph, 1);
        assert_eq!(order, vec![1, 2, 3, 4, 5]);
        let unique: HashSet<NodeId> = order.iter().copied().collect();
        assert_eq!(unique.len(), order.len());
        assert!(bfs(&graph, 999).is_empty());
        assert!(bfs(&CsrGraph::new(), 1).is_empty());
    }

    /// AC-0041-02 — dos islas producen exactamente dos componentes disjuntas.
    #[test]
    fn test_ac_0041_02_connected_components() {
        // Isla A: 1->2->3. Isla B con aristas invertidas (la dirección se ignora).
        let graph = graph_from(&[(1, 2), (2, 3), (11, 10), (12, 11)]);
        let components = connected_components(&graph);
        assert_eq!(components, vec![vec![1, 2, 3], vec![10, 11, 12]]);
        let flat: Vec<NodeId> = components.iter().flatten().copied().collect();
        let unique: HashSet<NodeId> = flat.iter().copied().collect();
        assert_eq!(flat.len(), graph.node_count());
        assert_eq!(unique.len(), graph.node_count());
        assert!(connected_components(&CsrGraph::new()).is_empty());
    }

    /// AC-0041-03 — el nodo más referenciado obtiene el score más alto.
    #[test]
    fn test_ac_0041_03_pagerank_ranks_hub() {
        // Estrella entrante: 2->1, 3->1, 4->1. El nodo 1 es hub y dangling.
        let graph = graph_from(&[(2, 1), (3, 1), (4, 1)]);
        let scores = pagerank(&graph, 0.85, 1000, 1e-12).expect("pagerank converge");
        assert_eq!(scores.len(), 4);
        // Punto fijo del modelo con redistribución de dangling: 71/131 y 20/131.
        let hub = scores[0];
        assert!((hub - 71.0 / 131.0).abs() < 1e-9, "hub={hub}");
        for score in &scores[1..] {
            assert!((score - 20.0 / 131.0).abs() < 1e-9, "score={score}");
            assert!(hub > *score);
        }
        assert!((total(&scores) - 1.0).abs() < 1e-9);
        assert!(scores.iter().all(|score| score.is_finite()));
        // La cota `max_iters` se respeta: con una iteración se devuelve el
        // primer vector de la potencia (no el punto fijo).
        let first = pagerank(&graph, 0.85, 1, 1e-12).expect("una iteración");
        assert!((first[0] - 0.728125).abs() < 1e-12, "first[0]={}", first[0]);
        for score in &first[1..] {
            assert!((score - 0.090625).abs() < 1e-12, "first={score}");
        }
    }

    /// PageRank reparte la masa según el grado de salida del predecesor.
    #[test]
    fn test_pagerank_uses_out_degree() {
        // Nodo 1 apunta a 2 y 3 (grado 2); 2 y 3 apuntan solo a 1.
        let graph = graph_from(&[(1, 2), (1, 3), (2, 1), (3, 1)]);
        let scores = pagerank(&graph, 0.85, 1000, 1e-12).expect("pagerank converge");
        assert!((scores[0] - 18.0 / 37.0).abs() < 1e-9, "p1={}", scores[0]);
        assert!((scores[1] - 19.0 / 74.0).abs() < 1e-9, "p2={}", scores[1]);
        assert!((scores[2] - 19.0 / 74.0).abs() < 1e-9, "p3={}", scores[2]);
        assert!((total(&scores) - 1.0).abs() < 1e-9);
    }

    /// AC-0041-04 — con un nodo colgante converge sin NaN y la suma es ~1.
    #[test]
    fn test_ac_0041_04_pagerank_dangling() {
        // Cadena 1->2->3 con el nodo 3 colgante (sin aristas de salida).
        let graph = graph_from(&[(1, 2), (2, 3)]);
        let scores = pagerank(&graph, 0.85, 1000, 1e-12).expect("pagerank converge");
        assert_eq!(scores.len(), 3);
        assert!(scores.iter().all(|score| score.is_finite()), "sin NaN");
        assert!(
            (total(&scores) - 1.0).abs() < 1e-9,
            "suma={}",
            total(&scores)
        );
        // El sumidero acumula más masa que sus predecesores.
        assert!(scores[2] > scores[1] && scores[1] > scores[0]);
        // Sin colgantes, el ciclo 1<->2 reparte la masa a partes iguales.
        let cycle = graph_from(&[(1, 2), (2, 1)]);
        let cycle_scores = pagerank(&cycle, 0.85, 1000, 1e-12).expect("ciclo converge");
        assert!((cycle_scores[0] - 0.5).abs() < 1e-9);
        assert!((cycle_scores[1] - 0.5).abs() < 1e-9);
    }

    /// AC-0041-05 — grafos frontera (vacío, un nodo, self-loop) y parámetros
    /// inválidos no provocan panic.
    #[test]
    fn test_ac_0041_05_boundary_graphs() {
        let empty = CsrGraph::new();
        assert!(bfs(&empty, 0).is_empty());
        assert!(connected_components(&empty).is_empty());
        assert!(
            pagerank(&empty, 0.85, 10, 1e-9)
                .expect("grafo vacío es válido")
                .is_empty()
        );

        let self_loop = graph_from(&[(1, 1)]);
        assert_eq!(bfs(&self_loop, 1), vec![1]);
        assert_eq!(connected_components(&self_loop), vec![vec![1]]);
        let self_scores = pagerank(&self_loop, 0.85, 100, 1e-12).expect("self-loop converge");
        assert!((self_scores[0] - 1.0).abs() < 1e-9);

        for damping in [0.0, 1.0, -0.5, f64::NAN] {
            assert!(matches!(
                pagerank(&self_loop, damping, 10, 1e-9),
                Err(RuscaError::InvalidConfig(_))
            ));
        }
        assert!(matches!(
            pagerank(&self_loop, 0.85, 0, 1e-9),
            Err(RuscaError::InvalidConfig(_))
        ));
        for tol in [0.0, -1.0, f64::NAN] {
            assert!(matches!(
                pagerank(&self_loop, 0.85, 10, tol),
                Err(RuscaError::InvalidConfig(_))
            ));
        }
    }

    proptest! {
        /// PBT — las componentes particionan exactamente todos los nodos.
        #[test]
        fn prop_components_partition_all_nodes(edges in edge_strategy()) {
            let graph = graph_from(&edges);
            let components = connected_components(&graph);
            let flat: Vec<NodeId> = components.iter().flatten().copied().collect();
            prop_assert_eq!(flat.len(), graph.node_count());
            let unique: HashSet<NodeId> = flat.iter().copied().collect();
            prop_assert_eq!(unique.len(), graph.node_count());
            for component in &components {
                prop_assert!(!component.is_empty());
                let mut sorted = component.clone();
                sorted.sort_unstable();
                prop_assert_eq!(component, &sorted);
            }
        }

        /// PBT — PageRank suma ~1, es finito y no negativo.
        #[test]
        fn prop_pagerank_normalized(edges in edge_strategy(), damping in 0.05f64..0.95) {
            let graph = graph_from(&edges);
            let scores = pagerank(&graph, damping, 200, 1e-10).expect("parámetros válidos");
            prop_assert_eq!(scores.len(), graph.node_count());
            if graph.node_count() > 0 {
                let sum = total(&scores);
                prop_assert!((sum - 1.0).abs() < 1e-6, "suma={}", sum);
                for score in &scores {
                    prop_assert!(score.is_finite());
                    prop_assert!(*score >= 0.0);
                }
            }
        }

        /// PBT — BFS solo visita nodos alcanzables y empieza por el origen.
        #[test]
        fn prop_bfs_visits_reachable(edges in edge_strategy(), start in 0u64..8) {
            let graph = graph_from(&edges);
            let order = bfs(&graph, start);
            if !graph.contains(start) {
                prop_assert!(order.is_empty());
            } else {
                prop_assert_eq!(order.first().copied(), Some(start));
                let visited: HashSet<NodeId> = order.iter().copied().collect();
                prop_assert_eq!(visited.len(), order.len());
                for node in &order {
                    prop_assert!(graph.contains(*node));
                }
            }
        }
    }
}
