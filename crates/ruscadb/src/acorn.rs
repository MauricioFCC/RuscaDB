//! Busqueda filtrada ACORN sobre grafo (SPEC-0172).
//!
//! ACORN (arXiv:2403.04871) recorre el grafo de HNSW **atravesando nodos que no
//! cumplen el predicado** para no desconectar la busqueda cuando el filtro es
//! selectivo (el llamado *predicate subgraph traversal*). Este modulo implementa
//! el recorrido best-first con ese comportamiento sobre una lista de adyacencia
//! explicita, de modo que la politica se puede probar sin el motor completo.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

/// Grafo no dirigido de vecinos (lista de adyacencia).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    /// `adjacency[node]` = vecinos de `node`.
    pub adjacency: Vec<Vec<usize>>,
}

impl Graph {
    /// Crea un grafo con `nodes` nodos aislados.
    ///
    /// Args:
    ///     nodes: Numero de nodos.
    ///
    /// Returns:
    ///     Grafo sin aristas.
    pub fn new(nodes: usize) -> Self {
        Self {
            adjacency: vec![Vec::new(); nodes],
        }
    }

    /// Arista bidireccional entre dos nodos.
    ///
    /// Args:
    ///     a: Nodo origen.
    ///     b: Nodo destino.
    ///
    /// Returns:
    ///     `&mut Self` para encadenar.
    pub fn connect(&mut self, a: usize, b: usize) -> &mut Self {
        for (from, to) in [(a, b), (b, a)] {
            if let Some(neighbors) = self.adjacency.get_mut(from)
                && !neighbors.contains(&to)
            {
                neighbors.push(to);
            }
        }
        self
    }

    /// Numero de nodos.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.adjacency.len()
    }

    /// Indica si el grafo esta vacio.
    ///
    /// Returns:
    ///     `true` sin nodos.
    pub fn is_empty(&self) -> bool {
        self.adjacency.is_empty()
    }
}

/// Par `(distancia, nodo)` ordenado para un heap de minimos.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Candidate {
    /// Distancia a la consulta.
    distance: f32,
    /// Identificador del nodo.
    node: usize,
}

impl Eq for Candidate {}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // Invertido: el heap de Rust es max-heap, queremos el mas cercano.
        other
            .distance
            .total_cmp(&self.distance)
            .then_with(|| other.node.cmp(&self.node))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Buscador filtrado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcornSearch {
    /// Factor de expansion del subgrafo de predicado.
    pub expand_factor: usize,
}

impl Default for AcornSearch {
    fn default() -> Self {
        Self { expand_factor: 4 }
    }
}

impl AcornSearch {
    /// Crea el buscador.
    ///
    /// Args:
    ///     expand_factor: Multiplicador de expansion (minimo 1).
    ///
    /// Returns:
    ///     Buscador configurado.
    pub fn new(expand_factor: usize) -> Self {
        Self {
            expand_factor: expand_factor.max(1),
        }
    }

    /// Decide si conviene expansion de subgrafo para una selectividad dada.
    ///
    /// Args:
    ///     selectivity: Fraccion estimada de filas que cumplen el predicado.
    ///
    /// Returns:
    ///     `true` si la selectividad es muy baja (< 5%): el filtro desconectaria
    ///     el grafo y hay que atravesar nodos no admitidos.
    pub fn should_expand(&self, selectivity: f32) -> bool {
        selectivity < 0.05
    }

    /// Presupuesto de candidatos a explorar (compatibilidad).
    ///
    /// Args:
    ///     candidates: Candidatos base.
    ///
    /// Returns:
    ///     Candidatos multiplicados por el factor de expansion.
    pub fn search(&self, candidates: usize) -> usize {
        candidates.saturating_mul(self.expand_factor)
    }

    /// Busqueda filtrada best-first con traversal de subgrafo de predicado.
    ///
    /// Args:
    ///     graph: Lista de adyacencia.
    ///     entry: Nodo de entrada.
    ///     allowed: Mascara del predicado (`true` = cumple).
    ///     k: Numero de resultados deseados.
    ///     distance: Distancia del nodo a la consulta (menor = mas cercano).
    ///
    /// Returns:
    ///     Hasta `k` nodos admitidos, del mas cercano al mas lejano.
    pub fn filtered_search(
        &self,
        graph: &Graph,
        entry: usize,
        allowed: &[bool],
        k: usize,
        distance: impl Fn(usize) -> f32,
    ) -> Vec<usize> {
        if k == 0 || graph.is_empty() || entry >= graph.len() {
            return Vec::new();
        }
        let mut visited: HashSet<usize> = HashSet::new();
        let mut queue: BinaryHeap<Candidate> = BinaryHeap::new();
        let mut results: Vec<usize> = Vec::new();
        let budget = self.search(k).saturating_mul(4).max(8);
        let mut expanded = 0usize;

        visited.insert(entry);
        queue.push(Candidate {
            distance: distance(entry),
            node: entry,
        });

        while let Some(Candidate { node, .. }) = queue.pop() {
            let is_allowed = allowed.get(node).copied().unwrap_or(false);
            if is_allowed && !results.contains(&node) {
                results.push(node);
                if results.len() == k {
                    break;
                }
            }

            // Traversal de subgrafo: se expanden tambien nodos filtrados, pero
            // con presupuesto acotado para no degradar a un full-scan.
            let expand = is_allowed || expanded < budget;
            if !expand {
                continue;
            }
            expanded += 1;
            for &neighbor in graph.adjacency.get(node).map(Vec::as_slice).unwrap_or(&[]) {
                if visited.insert(neighbor) {
                    queue.push(Candidate {
                        distance: distance(neighbor),
                        node: neighbor,
                    });
                }
            }
        }

        results.sort_by(|a, b| distance(*a).total_cmp(&distance(*b)).then_with(|| a.cmp(b)));
        results
    }
}

#[cfg(test)]
mod tests {
    use super::{AcornSearch, Graph};

    /// AC-0172-01: con filtro selectivo el traversal de subgrafo recupera los
    /// vecinos admitidos mas cercanos.
    #[test]
    // @spec AC-0172-01
    fn test_ac_0172_01_recall() {
        // Cadena 0-1-2-...-19; solo los pares estan admitidos.
        let mut graph = Graph::new(20);
        for node in 0..19 {
            graph.connect(node, node + 1).connect(node, node + 2);
        }
        let allowed: Vec<bool> = (0..20).map(|node| node % 2 == 0).collect();
        let searcher = AcornSearch::new(4);
        assert!(searcher.should_expand(0.01));

        let results = searcher.filtered_search(&graph, 0, &allowed, 3, |node| node as f32);
        assert_eq!(results, vec![0, 2, 4], "los pares mas cercanos");

        // Grafo completamente filtrado: no hay resultados, pero no cuelga.
        let none_allowed = vec![false; 20];
        assert!(
            searcher
                .filtered_search(&graph, 0, &none_allowed, 3, |node| node as f32)
                .is_empty()
        );

        // k = 0 y entrada invalida son no-op.
        assert!(
            searcher
                .filtered_search(&graph, 0, &allowed, 0, |node| node as f32)
                .is_empty()
        );
        assert!(
            searcher
                .filtered_search(&graph, 99, &allowed, 3, |node| node as f32)
                .is_empty()
        );
    }

    /// AC-0172-02: politica pre/in/post filtro segun selectividad.
    #[test]
    // @spec AC-0172-02
    fn test_ac_0172_02_prefilter() {
        let searcher = AcornSearch::new(4);
        assert!(!searcher.should_expand(0.5));
        assert!(!searcher.should_expand(0.05));
        assert!(searcher.should_expand(0.049));
        assert_eq!(searcher.search(10), 40);
        assert_eq!(AcornSearch::new(0).expand_factor, 1, "minimo 1");

        // Con selectividad alta (casi todo admitido) el resultado es exacto.
        let mut graph = Graph::new(6);
        for node in 0..5 {
            graph.connect(node, node + 1);
        }
        let allowed = vec![true; 6];
        let results =
            searcher.filtered_search(&graph, 3, &allowed, 2, |node| (node as f32 - 3.0).abs());
        assert_eq!(results, vec![3, 2]);
    }

    /// El grafo es no dirigido, sin duplicados, y las cotas son correctas.
    #[test]
    fn graph_connect_is_bidirectional_and_idempotent() {
        let mut graph = Graph::new(3);
        graph.connect(0, 1).connect(1, 0).connect(0, 2);
        assert_eq!(graph.adjacency[0], vec![1, 2]);
        assert_eq!(graph.adjacency[1], vec![0]);
        assert_eq!(graph.adjacency[2], vec![0]);
        assert_eq!(graph.len(), 3);
        assert!(!graph.is_empty());
        assert!(Graph::new(0).is_empty());
        assert_eq!(Graph::default(), Graph::new(0));
    }

    /// Una mascara de predicado mas corta que el grafo se trata como `false`.
    #[test]
    fn short_allowed_mask_is_safe() {
        let mut graph = Graph::new(4);
        graph.connect(0, 1).connect(1, 2).connect(2, 3);
        let searcher = AcornSearch::new(2);
        let results = searcher.filtered_search(&graph, 0, &[true, false], 3, |node| node as f32);
        assert_eq!(results, vec![0], "solo el nodo 0 esta admitido");
    }
}
