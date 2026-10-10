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
    /// Budget duro de RAM en bytes (`None` = sin límite, SPEC-0058).
    pub memory_budget_bytes: Option<u64>,
}

impl HnswParams {
    /// Crea parámetros con valores por defecto razonables (sin budget).
    ///
    /// Args:
    ///     metric: Métrica de distancia.
    pub fn new(metric: Metric) -> Self {
        Self {
            m: 16,
            ef_construction: 200,
            metric,
            memory_budget_bytes: None,
        }
    }
}

/// Estima el footprint RAM del HNSW en bytes (SPEC-0058, R3).
///
/// Modelo: `n·dim·4` (vectores f32) + `n·2·m·8` (vecinos capa 0, ids u64) +
/// `n·64` (overhead de nodo). Es una cota de admisión, no una medición.
///
/// Args:
///     n: Número de vectores.
///     m: Grado `HnswParams::m`.
///     dim: Dimensión.
///
/// Returns:
///     Bytes estimados.
pub fn estimate_footprint(n: usize, m: usize, dim: usize) -> u64 {
    n as u64 * (dim as u64 * 4 + 2 * m as u64 * 8 + 64)
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
    ///     [`RuscaError::DimensionMismatch`] si la dimensión no coincide;
    ///     [`RuscaError::ResourceLimit`] si el budget de RAM no admite otro vector.
    pub fn insert(&mut self, vector: &[f32]) -> Result<usize, RuscaError> {
        if let Some(budget) = self.params.memory_budget_bytes {
            let projected = estimate_footprint(self.nodes.len() + 1, self.params.m, self.dim);
            if projected > budget {
                return Err(RuscaError::ResourceLimit {
                    resource: "hnsw_ram".to_string(),
                    limit: budget,
                    actual: projected,
                });
            }
        }
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
                memory_budget_bytes: None,
            },
            3,
        )
        .expect("new");
        assert_eq!(index.max_connections(0), 16);
        assert_eq!(index.max_connections(1), 8);
    }
}

#[cfg(test)]
mod recall_tests {
    use std::cmp::Ordering;

    use super::*;

    /// Semilla fija del corpus determinista (SPEC-0034).
    const CORPUS_SEED: u64 = 0x0034_2026_1234_5678;
    /// Número de vectores indexados.
    const N_VECTORS: usize = 1000;
    /// Dimensión de los vectores.
    const DIM: usize = 16;
    /// Número de consultas de evaluación.
    const N_QUERIES: usize = 50;
    /// Vecinos recuperados (recall@10).
    const K: usize = 10;
    /// Amplitud de búsqueda en consulta, elegida para superar el objetivo.
    const EF_SEARCH: usize = 256;
    /// Umbral mínimo de aceptación del recall.
    const RECALL_TARGET: f64 = 0.95;

    /// Generador congruencial lineal (LCG) propio y determinista.
    struct Lcg {
        state: u64,
    }

    impl Lcg {
        /// Crea el LCG con una semilla no nula.
        fn new(seed: u64) -> Self {
            Self { state: seed | 1 }
        }

        /// Siguiente entero de 64 bits (constantes de Knuth).
        fn next_u64(&mut self) -> u64 {
            self.state = self
                .state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.state
        }

        /// Siguiente flotante uniforme en `[0, 1)`.
        fn next_unit(&mut self) -> f32 {
            ((self.next_u64() >> 40) as f32) / ((1u32 << 24) as f32)
        }
    }

    /// Vectores deterministas en `[-1, 1]` generados con el LCG propio.
    ///
    /// Args:
    ///     count: Número de vectores.
    ///     dim: Dimensión de cada vector.
    ///     seed: Semilla fija del generador.
    ///
    /// Returns:
    ///     La lista de vectores.
    fn lcg_vectors(count: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut rng = Lcg::new(seed);
        (0..count)
            .map(|_| (0..dim).map(|_| rng.next_unit() * 2.0 - 1.0).collect())
            .collect()
    }

    /// Construye un índice insertando los vectores en orden.
    ///
    /// Args:
    ///     vectors: Vectores no vacíos de igual dimensión.
    ///     metric: Métrica de distancia.
    ///
    /// Returns:
    ///     El índice HNSW poblado.
    fn build_index(vectors: &[Vec<f32>], metric: Metric) -> HnswIndex {
        let mut index = HnswIndex::new(HnswParams::new(metric), vectors[0].len()).expect("new");
        for vector in vectors {
            index.insert(vector).expect("insert");
        }
        index
    }

    /// Oráculo de fuerza bruta: ids de los `k` más cercanos.
    ///
    /// Ordena por distancia ascendente y desempata por índice.
    ///
    /// Args:
    ///     vectors: Corpus indexado.
    ///     query: Vector de consulta.
    ///     k: Número de vecinos exactos.
    ///     metric: Métrica de distancia.
    ///
    /// Returns:
    ///     Los ids del top-k exacto.
    fn brute_force(vectors: &[Vec<f32>], query: &[f32], k: usize, metric: Metric) -> Vec<usize> {
        let mut scored: Vec<(usize, f32)> = vectors
            .iter()
            .enumerate()
            .map(|(id, vector)| {
                let distance = crate::distance::distance(metric, query, vector).expect("dist");
                (id, distance)
            })
            .collect();
        scored.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        scored.into_iter().take(k).map(|(id, _)| id).collect()
    }

    /// Recall@k promedio de HNSW frente al oráculo de fuerza bruta.
    ///
    /// Args:
    ///     index: Índice HNSW a evaluar.
    ///     vectors: Corpus indexado (mismo orden de inserción).
    ///     queries: Consultas de evaluación.
    ///     k: Número de vecinos recuperados y exactos.
    ///     metric: Métrica de distancia.
    ///
    /// Returns:
    ///     El promedio de `|intersec| / k` sobre las consultas.
    fn recall_at_k(
        index: &HnswIndex,
        vectors: &[Vec<f32>],
        queries: &[Vec<f32>],
        k: usize,
        metric: Metric,
    ) -> f64 {
        if queries.is_empty() || k == 0 {
            return 1.0;
        }
        let total: f64 = queries
            .iter()
            .map(|query| {
                let expected = brute_force(vectors, query, k, metric);
                let got: Vec<usize> = index
                    .search(query, k, EF_SEARCH)
                    .expect("search")
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect();
                let hits = expected.iter().filter(|id| got.contains(id)).count();
                hits as f64 / k as f64
            })
            .sum();
        total / queries.len() as f64
    }

    /// AC-0034-01 — recall@10 de HNSW frente a fuerza bruta >= 0.95.
    #[test]
    // @spec AC-0034-01
    fn test_ac_0034_01_recall_at_10_meets_target() {
        let vectors = lcg_vectors(N_VECTORS, DIM, CORPUS_SEED);
        let queries = lcg_vectors(N_QUERIES, DIM, CORPUS_SEED ^ 0xDEAD_BEEF);
        let index = build_index(&vectors, Metric::L2);
        let average = recall_at_k(&index, &vectors, &queries, K, Metric::L2);
        println!(
            "SPEC-0034 recall@{K} = {average:.4} (N={N_VECTORS}, M={N_QUERIES}, ef_search={EF_SEARCH})"
        );
        assert!(
            average >= RECALL_TARGET,
            "recall@{K} = {average:.4} (objetivo >= {RECALL_TARGET}, ef_search = {EF_SEARCH})"
        );
    }

    /// AC-0034-02 — dos búsquedas idénticas devuelven el mismo resultado.
    #[test]
    // @spec AC-0034-02
    fn test_ac_0034_02_search_is_deterministic() {
        let vectors = lcg_vectors(200, DIM, CORPUS_SEED);
        let index = build_index(&vectors, Metric::L2);
        let query = lcg_vectors(1, DIM, 7).remove(0);
        let first = index.search(&query, K, EF_SEARCH).expect("first");
        let second = index.search(&query, K, EF_SEARCH).expect("second");
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            assert_eq!(a.0, b.0, "ids distintos entre búsquedas");
            assert_eq!(a.1.total_cmp(&b.1), Ordering::Equal, "distancias distintas");
        }
    }

    /// AC-0034-03 — índice vacío o `k == 0` devuelven vacío sin panics.
    #[test]
    // @spec AC-0034-03
    fn test_ac_0034_03_empty_and_zero_k_are_safe() {
        let empty = HnswIndex::new(HnswParams::new(Metric::L2), DIM).expect("new");
        let empty_results = empty.search(&[0.0; DIM], K, EF_SEARCH).expect("empty");
        assert!(empty_results.is_empty());

        let vectors = lcg_vectors(32, DIM, CORPUS_SEED);
        let index = build_index(&vectors, Metric::L2);
        let zero_k = index.search(&vectors[0], 0, EF_SEARCH).expect("zero k");
        assert!(zero_k.is_empty());
    }
}
