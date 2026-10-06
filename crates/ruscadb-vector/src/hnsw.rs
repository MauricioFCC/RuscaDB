//! Índice HNSW (Hierarchical Navigable Small World).

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use ruscadb_core::{Metric, RuscaError};

use crate::distance::raw_distance;
use crate::rng::Rng;

/// Parámetros de construcción de un índice HNSW.
#[derive(Clone, Copy, Debug)]
pub struct HnswParams {
    /// Grado máximo por nodo en capas superiores (capa 0 usa `2 * m`).
    pub m: usize,
    /// Amplitud de búsqueda durante la construcción.
    pub ef_construction: usize,
    /// Métrica de distancia.
    pub metric: Metric,
}

impl HnswParams {
    /// Crea parámetros con valores por defecto razonables.
    ///
    /// Args:
    ///     metric: Métrica de distancia.
    pub fn new(metric: Metric) -> Self {
        Self {
            m: 16,
            ef_construction: 200,
            metric,
        }
    }
}

/// Candidato (nodo, distancia) ordenable por distancia.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    distance: f32,
    id: usize,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.distance.total_cmp(&other.distance) == Ordering::Equal && self.id == other.id
    }
}

impl Eq for Candidate {}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Nodo del grafo HNSW.
struct Node {
    vector: Vec<f32>,
    neighbors: Vec<Vec<usize>>,
}

/// Índice vectorial HNSW.
pub struct HnswIndex {
    params: HnswParams,
    dim: usize,
    nodes: Vec<Node>,
    entry: Option<usize>,
    top_level: usize,
    rng: Rng,
}

impl HnswIndex {
    /// Crea un índice vacío.
    ///
    /// Args:
    ///     params: Parámetros HNSW.
    ///     dim: Dimensión de los vectores (>= 1).
    ///
    /// Returns:
    ///     El índice vacío.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si `dim == 0`, `m < 2` o
    ///     `ef_construction == 0`.
    pub fn new(params: HnswParams, dim: usize) -> Result<Self, RuscaError> {
        if dim == 0 {
            return Err(RuscaError::InvalidConfig(
                "la dimensión del índice debe ser >= 1".to_string(),
            ));
        }
        if params.m < 2 {
            return Err(RuscaError::InvalidConfig(
                "m debe ser >= 2 para el nivel geométrico".to_string(),
            ));
        }
        if params.ef_construction == 0 {
            return Err(RuscaError::InvalidConfig(
                "ef_construction debe ser >= 1".to_string(),
            ));
        }
        Ok(Self {
            params,
            dim,
            nodes: Vec::new(),
            entry: None,
            top_level: 0,
            rng: Rng::new(0x9E37_79B9_7F4A_7C15),
        })
    }

    /// Dimensión de los vectores del índice.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Número de vectores indexados.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` si el índice está vacío.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Inserta un vector y devuelve su id de nodo.
    ///
    /// Args:
    ///     vector: Vector de dimensión `dim`.
    ///
    /// Returns:
    ///     El id asignado (0-based, en orden de inserción).
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si la dimensión no coincide.
    pub fn insert(&mut self, vector: &[f32]) -> Result<usize, RuscaError> {
        self.insert_impl(vector)
    }

    /// Busca los `k` vecinos más cercanos.
    ///
    /// Args:
    ///     query: Vector de consulta de dimensión `dim`.
    ///     k: Número de vecinos a devolver.
    ///     ef_search: Amplitud de búsqueda (se usa `max(ef_search, k)`).
    ///
    /// Returns:
    ///     Lista de `(id, distancia)` ordenada por distancia ascendente.
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si la dimensión no coincide.
    pub fn search(
        &self,
        query: &[f32],
        k: usize,
        ef_search: usize,
    ) -> Result<Vec<(usize, f32)>, RuscaError> {
        self.search_impl(query, k, ef_search)
    }

    /// Implementación real de `insert`.
    fn insert_impl(&mut self, vector: &[f32]) -> Result<usize, RuscaError> {
        if vector.len() != self.dim {
            return Err(RuscaError::DimensionMismatch {
                expected: self.dim,
                actual: vector.len(),
            });
        }
        let id = self.nodes.len();
        let level = self.rng.random_level(self.params.m);
        self.nodes.push(Node {
            vector: vector.to_vec(),
            neighbors: vec![Vec::new(); level + 1],
        });

        let Some(entry) = self.entry else {
            self.entry = Some(id);
            self.top_level = level;
            return Ok(id);
        };

        let mut entry_point = entry;
        let mut layer = self.top_level;
        while layer > level {
            entry_point = self.greedy(vector, entry_point, layer);
            layer -= 1;
        }

        let mut entry_points = vec![entry_point];
        for current in (0..=level.min(self.top_level)).rev() {
            let candidates =
                self.search_layer(vector, &entry_points, self.params.ef_construction, current);
            self.connect(id, current, &candidates);
            entry_points = candidates.iter().take(1).map(|(node, _)| *node).collect();
            if entry_points.is_empty() {
                entry_points = vec![entry_point];
            }
        }

        if level > self.top_level {
            self.entry = Some(id);
            self.top_level = level;
        }
        Ok(id)
    }

    /// Implementación real de `search`.
    fn search_impl(
        &self,
        query: &[f32],
        k: usize,
        ef_search: usize,
    ) -> Result<Vec<(usize, f32)>, RuscaError> {
        if query.len() != self.dim {
            return Err(RuscaError::DimensionMismatch {
                expected: self.dim,
                actual: query.len(),
            });
        }
        let Some(entry) = self.entry else {
            return Ok(Vec::new());
        };
        if k == 0 {
            return Ok(Vec::new());
        }

        let mut entry_point = entry;
        let mut layer = self.top_level;
        while layer > 0 {
            entry_point = self.greedy(query, entry_point, layer);
            layer -= 1;
        }

        let mut results = self.search_layer(query, &[entry_point], ef_search.max(k), 0);
        results.truncate(k);
        Ok(results)
    }

    /// Número máximo de conexiones en una capa (`2m` en la capa 0).
    fn max_connections(&self, layer: usize) -> usize {
        if layer == 0 {
            2 * self.params.m
        } else {
            self.params.m
        }
    }

    /// Búsqueda voraz en una capa superior (ef=1) hasta un mínimo local.
    fn greedy(&self, query: &[f32], entry: usize, layer: usize) -> usize {
        let mut current = entry;
        let mut current_distance =
            raw_distance(self.params.metric, query, &self.nodes[current].vector);
        loop {
            let mut improved = false;
            if let Some(neighbors) = self.nodes[current].neighbors.get(layer) {
                for &neighbor in neighbors {
                    let distance =
                        raw_distance(self.params.metric, query, &self.nodes[neighbor].vector);
                    if distance < current_distance {
                        current = neighbor;
                        current_distance = distance;
                        improved = true;
                    }
                }
            }
            if !improved {
                return current;
            }
        }
    }

    /// Búsqueda best-first en una capa con amplitud `ef`.
    fn search_layer(
        &self,
        query: &[f32],
        entries: &[usize],
        ef: usize,
        layer: usize,
    ) -> Vec<(usize, f32)> {
        let mut visited = vec![false; self.nodes.len()];
        let mut candidates: BinaryHeap<std::cmp::Reverse<Candidate>> = BinaryHeap::new();
        let mut results: BinaryHeap<Candidate> = BinaryHeap::new();

        for &entry in entries {
            if visited[entry] {
                continue;
            }
            visited[entry] = true;
            let distance = raw_distance(self.params.metric, query, &self.nodes[entry].vector);
            candidates.push(std::cmp::Reverse(Candidate {
                distance,
                id: entry,
            }));
            results.push(Candidate {
                distance,
                id: entry,
            });
        }

        while let Some(std::cmp::Reverse(current)) = candidates.pop() {
            if results.len() >= ef
                && let Some(farthest) = results.peek()
                && current.distance > farthest.distance
            {
                break;
            }
            let Some(neighbors) = self.nodes[current.id].neighbors.get(layer) else {
                continue;
            };
            for &neighbor in neighbors {
                if visited[neighbor] {
                    continue;
                }
                visited[neighbor] = true;
                let distance =
                    raw_distance(self.params.metric, query, &self.nodes[neighbor].vector);
                let farthest = results.peek().map_or(f32::INFINITY, |c| c.distance);
                if results.len() < ef || distance < farthest {
                    candidates.push(std::cmp::Reverse(Candidate {
                        distance,
                        id: neighbor,
                    }));
                    results.push(Candidate {
                        distance,
                        id: neighbor,
                    });
                    if results.len() > ef {
                        results.pop();
                    }
                }
            }
        }

        let mut output: Vec<(usize, f32)> =
            results.into_iter().map(|c| (c.id, c.distance)).collect();
        output.sort_by(|a, b| a.1.total_cmp(&b.1));
        output
    }

    /// Conecta `id` con los mejores candidatos y poda las listas de vecinos.
    fn connect(&mut self, id: usize, layer: usize, candidates: &[(usize, f32)]) {
        let max = self.max_connections(layer);
        let selected: Vec<usize> = candidates.iter().take(max).map(|(node, _)| *node).collect();
        self.nodes[id].neighbors[layer] = selected.clone();

        for &neighbor in &selected {
            let mut list = self.nodes[neighbor].neighbors[layer].clone();
            if !list.contains(&id) {
                list.push(id);
            }
            if list.len() > max {
                let neighbor_vector = self.nodes[neighbor].vector.clone();
                let mut scored: Vec<(usize, f32)> = list
                    .iter()
                    .map(|&node| {
                        (
                            node,
                            raw_distance(
                                self.params.metric,
                                &neighbor_vector,
                                &self.nodes[node].vector,
                            ),
                        )
                    })
                    .collect();
                scored.sort_by(|a, b| a.1.total_cmp(&b.1));
                scored.truncate(max);
                list = scored.into_iter().map(|(node, _)| node).collect();
            }
            self.nodes[neighbor].neighbors[layer] = list;
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use std::cmp::Ordering;

    /// `Candidate` compara por distancia y luego por id.
    #[test]
    fn candidate_equality_and_order() {
        let a = Candidate {
            distance: 1.0,
            id: 0,
        };
        let b = Candidate {
            distance: 1.0,
            id: 0,
        };
        let c = Candidate {
            distance: 2.0,
            id: 0,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.cmp(&c), Ordering::Less);
        assert_eq!(c.cmp(&a), Ordering::Greater);
        assert_eq!(a.cmp(&b), Ordering::Equal);
    }

    /// La capa 0 duplica el grado máximo.
    #[test]
    fn max_connections_doubles_at_layer_zero() {
        let index = HnswIndex::new(
            HnswParams {
                m: 8,
                ef_construction: 10,
                metric: Metric::L2,
            },
            3,
        )
        .expect("new");
        assert_eq!(index.max_connections(0), 16);
        assert_eq!(index.max_connections(1), 8);
    }
}
