//! IVF-PQ real: cuantizacion por producto con k-means por subespacio (SPEC-0125).
//!
//! Sustituye el stub de "promedio por subespacio" por un indice IVF-PQ:
//!
//! 1. **Coarse quantizer**: k-means sobre los vectores completos -> `C` centroides;
//!    cada vector cae en una lista invertida.
//! 2. **Product quantization**: el vector se parte en `S` subespacios; cada
//!    subespacio tiene su propio codebook de hasta 256 centroides (k-means).
//!    Un vector se codifica en `S` bytes (uno por subespacio).
//! 3. **Busqueda ADC**: se exploran solo `nprobe` listas (las de centroides
//!    gruesos mas cercanos) y se estima la distancia con la tabla
//!    query->centroide por subespacio; opcionalmente se re-rankean los
//!    candidatos con el vector exacto.
//!
//! Complejidad: entrenar es `O(iters * n * k * d)` (**lineal en n**); buscar es
//! `O(C*d + nprobe * list_size * S + rerank)`, **sublineal** en n cuando
//! `nprobe << C`. No hay ningun paso `O(n^2)`.
//!
//! Referencias: Jegou et al., "Product Quantization for Nearest Neighbor
//! Search", TPAMI 2011; Sivic & Zisserman (IVF), ICCV 2003.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use ruscadb_core::RuscaError;

/// Maximo de centroides por subespacio (el codigo es 1 byte).
const MAX_CENTROIDS: usize = 256;

/// Metadatos de una busqueda (para verificar la cota de trabajo).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Vectores puntuados con ADC.
    pub scored: usize,
    /// Candidatos re-rankeados con distancia exacta.
    pub reranked: usize,
    /// Listas invertidas exploradas.
    pub probes: usize,
}

/// Par `(distancia, id)` ordenado para un heap de maximos.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Neighbor {
    distance: f32,
    id: usize,
}

impl Eq for Neighbor {}

impl Ord for Neighbor {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialOrd for Neighbor {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Generador determinista (xorshift64*) para inicializar k-means.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % bound as u64) as usize
        }
    }
}

/// Configuracion calibrada para una carga real (nprobe + oversampling).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    /// Listas IVF a explorar.
    pub nprobe: usize,
    /// Candidatos = `k * rerank_factor` antes del re-rank exacto.
    pub rerank_factor: usize,
    /// Recall@k medido sobre las consultas de calibracion.
    pub recall: f64,
    /// Vectores puntuados por consulta (media).
    pub mean_scored: f64,
}

/// Indice IVF-PQ.
#[derive(Debug, Clone)]
pub struct IvfPqIndex {
    num_subspaces: usize,
    subspace_dim: usize,
    dimension: usize,
    centroids_per_subspace: usize,
    /// `codebooks[s]` = `centroids_per_subspace * subspace_dim` floats.
    codebooks: Vec<Vec<f32>>,
    coarse_centroids: Vec<Vec<f32>>,
    /// Listas invertidas: cluster -> ids de vector.
    lists: Vec<Vec<usize>>,
    /// Codigos: un byte por subespacio y vector.
    codes: Vec<Vec<u8>>,
    /// Vectores exactos (para re-rank); vacio si `store_exact = false`.
    exact: Vec<Vec<f32>>,
    store_exact: bool,
    rerank_factor: usize,
    trained: bool,
}

impl IvfPqIndex {
    /// Crea un indice sin entrenar.
    ///
    /// Args:
    ///     num_subspaces: Numero de subespacios (`S`).
    ///     subspace_dim: Dimension de cada subespacio.
    ///
    /// Returns:
    ///     Indice vacio; `train` fija los codebooks.
    pub fn new(num_subspaces: usize, subspace_dim: usize) -> Self {
        Self {
            num_subspaces,
            subspace_dim,
            dimension: num_subspaces.saturating_mul(subspace_dim),
            centroids_per_subspace: 1,
            codebooks: Vec::new(),
            coarse_centroids: Vec::new(),
            lists: Vec::new(),
            codes: Vec::new(),
            exact: Vec::new(),
            store_exact: true,
            rerank_factor: 20,
            trained: false,
        }
    }

    /// Entrena codebooks y cuantizador grueso.
    ///
    /// Args:
    ///     vectors: Corpus de entrenamiento (misma dimension).
    ///     centroids_per_subspace: Centroides por subespacio (`1..=256`).
    ///     coarse_clusters: Clusters del IVF (0 o 1 = lista unica).
    ///     iterations: Iteraciones de Lloyd.
    ///
    /// Returns:
    ///     `Ok(())` si el corpus es valido.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el corpus esta vacio, las
    ///     dimensiones no cuadran o los centroides estan fuera de rango.
    pub fn train(
        &mut self,
        vectors: &[Vec<f32>],
        centroids_per_subspace: usize,
        coarse_clusters: usize,
        iterations: usize,
    ) -> Result<(), RuscaError> {
        if vectors.is_empty() {
            return Err(RuscaError::InvalidConfig(
                "IVF-PQ: corpus de entrenamiento vacio".to_string(),
            ));
        }
        if centroids_per_subspace == 0 || centroids_per_subspace > MAX_CENTROIDS {
            return Err(RuscaError::InvalidConfig(format!(
                "IVF-PQ: centroides por subespacio fuera de 1..={MAX_CENTROIDS}"
            )));
        }
        let dimension = vectors[0].len();
        if self.num_subspaces == 0
            || self.subspace_dim == 0
            || self.num_subspaces * self.subspace_dim != dimension
        {
            return Err(RuscaError::InvalidConfig(format!(
                "IVF-PQ: {dimension} dims no cuadran con {0} subespacios x {1}",
                self.num_subspaces, self.subspace_dim
            )));
        }
        if vectors.iter().any(|vector| vector.len() != dimension) {
            return Err(RuscaError::InvalidConfig(
                "IVF-PQ: vectores de dimension inconsistente".to_string(),
            ));
        }
        self.dimension = dimension;
        self.centroids_per_subspace = centroids_per_subspace;
        let iterations = iterations.max(1);

        // Codebooks por subespacio (k-means en el subespacio).
        let mut codebooks = Vec::with_capacity(self.num_subspaces);
        for subspace in 0..self.num_subspaces {
            let start = subspace * self.subspace_dim;
            let end = start + self.subspace_dim;
            let points: Vec<&[f32]> = vectors.iter().map(|vector| &vector[start..end]).collect();
            let centroids = kmeans(
                &points,
                centroids_per_subspace,
                iterations,
                0x9E37_79B9_7F4A_7C15 ^ (subspace as u64).wrapping_mul(0x1000_0001),
            );
            codebooks.push(centroids);
        }
        self.codebooks = codebooks;

        // Cuantizador grueso (IVF).
        let clusters = coarse_clusters.max(1).min(vectors.len());
        if clusters <= 1 {
            self.coarse_centroids = Vec::new();
            self.lists = vec![Vec::new()];
        } else {
            let points: Vec<&[f32]> = vectors.iter().map(Vec::as_slice).collect();
            let centroids = kmeans(&points, clusters, iterations.min(10), 0xD1B5_4A32_D192_ED03);
            self.coarse_centroids = centroids.chunks(dimension).map(<[f32]>::to_vec).collect();
            self.lists = vec![Vec::new(); self.coarse_centroids.len()];
        }

        // Reindexa el corpus con los codebooks recien entrenados.
        self.codes.clear();
        self.exact.clear();
        for list in &mut self.lists {
            list.clear();
        }
        self.trained = true;
        for vector in vectors {
            self.add(vector);
        }
        Ok(())
    }

    /// Ajusta el factor de oversampling del re-rank (minimo 1).
    ///
    /// Args:
    ///     factor: Candidatos = k * factor antes del re-rank exacto.
    pub fn set_rerank_factor(&mut self, factor: usize) {
        self.rerank_factor = factor.max(1);
    }

    /// Activa/desactiva el almacenamiento de vectores exactos para re-rank.
    ///
    /// Args:
    ///     enabled: `true` guarda los float32 (mas memoria, mejor recall).
    pub fn set_store_exact(&mut self, enabled: bool) {
        self.store_exact = enabled;
        if !enabled {
            self.exact.clear();
        }
    }

    /// Cuantiza un vector a `S` bytes (un centroide por subespacio).
    ///
    /// Args:
    ///     vector: Vector de `S * subspace_dim` floats.
    ///
    /// Returns:
    ///     Codigo de `num_subspaces` bytes (ceros si no esta entrenado).
    pub fn quantize(&self, vector: &[f32]) -> Vec<u8> {
        let mut code = vec![0u8; self.num_subspaces];
        if !self.trained {
            return code;
        }
        for (subspace, slot) in code.iter_mut().enumerate() {
            let start = subspace * self.subspace_dim;
            let end = (start + self.subspace_dim).min(vector.len());
            if start >= end {
                continue;
            }
            *slot = nearest_centroid(
                &vector[start..end],
                &self.codebooks[subspace],
                self.subspace_dim,
            ) as u8;
        }
        code
    }

    /// Anade un vector al indice (lo entrena con 1 centroide si hace falta).
    ///
    /// Args:
    ///     vector: Vector de la dimension del indice.
    pub fn add(&mut self, vector: &[f32]) {
        if !self.trained {
            // Fallback determinista: un unico centroide por subespacio.
            // `train` ya indexa el corpus, asi que no se anade dos veces.
            let corpus = vec![vector.to_vec()];
            let _ = self.train(&corpus, 1, 1, 1);
            return;
        }
        let id = self.codes.len();
        let code = self.quantize(vector);
        let cluster = self.assign_cluster(vector);
        if self.lists.is_empty() {
            self.lists.push(Vec::new());
        }
        self.lists[cluster].push(id);
        self.codes.push(code);
        if self.store_exact {
            self.exact.push(vector.to_vec());
        }
    }

    /// Numero de vectores indexados.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// Indica si el indice esta vacio.
    ///
    /// Returns:
    ///     `true` sin vectores.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// Numero de subespacios.
    ///
    /// Returns:
    ///     `S`.
    pub fn num_subspaces(&self) -> usize {
        self.num_subspaces
    }

    /// Indica si los codebooks estan entrenados.
    ///
    /// Returns:
    ///     `true` tras `train`.
    pub fn is_trained(&self) -> bool {
        self.trained
    }

    /// Busqueda IVF-PQ con ADC y re-rank exacto opcional.
    ///
    /// Args:
    ///     query: Vector de consulta.
    ///     k: Resultados deseados.
    ///     nprobe: Listas invertidas a explorar (>= 1).
    ///
    /// Returns:
    ///     Hasta `k` pares `(id, distancia)` por distancia ascendente.
    pub fn search(&self, query: &[f32], k: usize, nprobe: usize) -> Vec<(usize, f32)> {
        self.search_with_stats(query, k, nprobe).0
    }

    /// Igual que [`Self::search`] pero devuelve las metricas de trabajo.
    ///
    /// Args:
    ///     query: Vector de consulta.
    ///     k: Resultados deseados.
    ///     nprobe: Listas a explorar.
    ///
    /// Returns:
    ///     `(resultados, metricas)`.
    pub fn search_with_stats(
        &self,
        query: &[f32],
        k: usize,
        nprobe: usize,
    ) -> (Vec<(usize, f32)>, SearchStats) {
        self.search_internal(query, k, nprobe, self.rerank_factor)
    }

    /// Nucleo de busqueda con oversampling explicito (usado por `calibrate`).
    fn search_internal(
        &self,
        query: &[f32],
        k: usize,
        nprobe: usize,
        rerank_factor: usize,
    ) -> (Vec<(usize, f32)>, SearchStats) {
        let mut stats = SearchStats::default();
        if !self.trained || k == 0 || self.codes.is_empty() || query.len() != self.dimension {
            return (Vec::new(), stats);
        }

        // 1) Elegir listas por distancia al centroide grueso.
        let probes: Vec<usize> = if self.coarse_centroids.is_empty() {
            vec![0]
        } else {
            let mut ranked: Vec<(f32, usize)> = self
                .coarse_centroids
                .iter()
                .enumerate()
                .map(|(index, centroid)| (l2(query, centroid), index))
                .collect();
            ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            ranked
                .into_iter()
                .take(nprobe.max(1))
                .map(|(_, index)| index)
                .collect()
        };
        stats.probes = probes.len();

        // 2) Tabla ADC: distancia de cada subvector de la query a cada centroide.
        let mut table = vec![vec![0f32; self.centroids_per_subspace]; self.num_subspaces];
        for (subspace, row) in table.iter_mut().enumerate() {
            let start = subspace * self.subspace_dim;
            let end = start + self.subspace_dim;
            for (centroid, slot) in row.iter_mut().enumerate() {
                let base = centroid * self.subspace_dim;
                *slot = l2(
                    &query[start..end],
                    &self.codebooks[subspace][base..base + self.subspace_dim],
                );
            }
        }

        // 3) Puntuar solo las listas exploradas, con un heap acotado.
        // Oversampling configurable para el re-rank (sigue siendo sublineal).
        let keep = k.saturating_mul(rerank_factor).max(k);
        let mut heap: BinaryHeap<Neighbor> = BinaryHeap::with_capacity(keep + 1);
        for &cluster in &probes {
            let Some(list) = self.lists.get(cluster) else {
                continue;
            };
            for &id in list {
                let Some(code) = self.codes.get(id) else {
                    continue;
                };
                let mut distance = 0.0f32;
                for (subspace, &centroid) in code.iter().enumerate() {
                    distance += table[subspace][usize::from(centroid)];
                }
                stats.scored += 1;
                heap.push(Neighbor { distance, id });
                if heap.len() > keep {
                    heap.pop();
                }
            }
        }

        // 4) Re-rank exacto si guardamos los vectores originales.
        let mut candidates: Vec<Neighbor> = heap.into_vec();
        if self.store_exact && !self.exact.is_empty() {
            for candidate in &mut candidates {
                if let Some(vector) = self.exact.get(candidate.id) {
                    candidate.distance = l2(query, vector);
                    stats.reranked += 1;
                }
            }
        }
        candidates.sort_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then_with(|| a.id.cmp(&b.id))
        });
        candidates.truncate(k);
        (
            candidates
                .into_iter()
                .map(|neighbor| (neighbor.id, neighbor.distance))
                .collect(),
            stats,
        )
    }

    /// Calibra `nprobe` y el oversampling para una carga real.
    ///
    /// Recorre una rejilla geometrica de `nprobe` (1, 2, 4, ... hasta el numero
    /// de clusters) y varios factores de oversampling; elige la configuracion
    /// **mas barata** (menos vectores puntuados) que alcance `target_recall` sin
    /// superar `max_scored`.
    ///
    /// Args:
    ///     queries: Consultas de calibracion (misma dimension).
    ///     truth: Ground truth por consulta (ids del top-k exacto).
    ///     k: `k` del recall.
    ///     target_recall: Recall minimo, en `0..=1`.
    ///     max_scored: Tope de vectores puntuados por consulta (0 = sin tope).
    ///
    /// Returns:
    ///     La configuracion elegida, o `None` si ninguna cumple el objetivo.
    pub fn calibrate(
        &self,
        queries: &[Vec<f32>],
        truth: &[Vec<usize>],
        k: usize,
        target_recall: f64,
        max_scored: usize,
    ) -> Option<Calibration> {
        if queries.is_empty() || queries.len() != truth.len() || k == 0 {
            return None;
        }
        let clusters = if self.coarse_centroids.is_empty() {
            1
        } else {
            self.coarse_centroids.len()
        };
        let mut probes: Vec<usize> = Vec::new();
        let mut probe = 1usize;
        while probe < clusters {
            probes.push(probe);
            probe = probe.saturating_mul(2);
        }
        probes.push(clusters);
        probes.sort_unstable();
        probes.dedup();

        let limit = if max_scored == 0 {
            f64::INFINITY
        } else {
            max_scored as f64
        };
        let mut best: Option<Calibration> = None;
        for nprobe in probes {
            for rerank_factor in [4usize, 8, 20, 50] {
                let mut hits = 0usize;
                let mut total = 0usize;
                let mut scored = 0usize;
                for (query, expected) in queries.iter().zip(truth) {
                    let (found, stats) = self.search_internal(query, k, nprobe, rerank_factor);
                    scored += stats.scored;
                    hits += found.iter().filter(|(id, _)| expected.contains(id)).count();
                    total += expected.len();
                }
                if total == 0 {
                    continue;
                }
                let recall = hits as f64 / total as f64;
                let mean_scored = scored as f64 / queries.len() as f64;
                if recall < target_recall || mean_scored > limit {
                    continue;
                }
                let candidate = Calibration {
                    nprobe,
                    rerank_factor,
                    recall,
                    mean_scored,
                };
                let better = best.is_none_or(|current| {
                    candidate.mean_scored < current.mean_scored
                        || (candidate.mean_scored == current.mean_scored
                            && (candidate.nprobe, candidate.rerank_factor)
                                < (current.nprobe, current.rerank_factor))
                });
                if better {
                    best = Some(candidate);
                }
            }
        }
        best
    }

    /// Memoria estimada `(codigos, float32 equivalentes)`.
    ///
    /// Returns:
    ///     Bytes de codigos y bytes que ocuparian los vectores exactos.
    pub fn memory_report(&self) -> (usize, usize) {
        let code_bytes: usize = self.codes.len() * self.num_subspaces
            + self.lists.iter().map(Vec::len).sum::<usize>() * 8;
        let exact_bytes = self.codes.len() * self.dimension * 4;
        (code_bytes, exact_bytes)
    }

    /// Cluster IVF de un vector (0 si no hay cuantizador grueso).
    fn assign_cluster(&self, vector: &[f32]) -> usize {
        if self.coarse_centroids.is_empty() {
            return 0;
        }
        let mut best = 0usize;
        let mut best_distance = f32::INFINITY;
        for (index, centroid) in self.coarse_centroids.iter().enumerate() {
            let distance = l2(vector, centroid);
            if distance < best_distance {
                best_distance = distance;
                best = index;
            }
        }
        best
    }
}

/// Distancia L2 al cuadrado.
fn l2(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Indice del centroide mas cercano a `point`.
fn nearest_centroid(point: &[f32], centroids: &[f32], dim: usize) -> usize {
    let mut best = 0usize;
    let mut best_distance = f32::INFINITY;
    for (index, centroid) in centroids.chunks(dim).enumerate() {
        let distance = l2(point, centroid);
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}

/// k-means (Lloyd) determinista sobre `points`.
///
/// Args:
///     points: Puntos de igual dimension.
///     k: Centroides (1..=256).
///     iterations: Iteraciones maximas.
///     seed: Semilla del init determinista.
///
/// Returns:
///     Centroides aplanados `k * dim` (rellena con puntos reales si k > n).
fn kmeans(points: &[&[f32]], k: usize, iterations: usize, seed: u64) -> Vec<f32> {
    let dim = points.first().map_or(0, |point| point.len());
    let k = k.clamp(1, MAX_CENTROIDS);
    let mut centroids = vec![0f32; k * dim];
    if points.is_empty() || dim == 0 {
        return centroids;
    }
    let mut rng = Rng::new(seed);
    // Init por muestreo uniforme determinista (sin reemplazo cuando n >= k).
    let mut chosen: Vec<usize> = Vec::with_capacity(k);
    for index in 0..k {
        let candidate = if points.len() >= k {
            (index * points.len()) / k
        } else {
            rng.below(points.len())
        };
        chosen.push(candidate);
        centroids[index * dim..(index + 1) * dim].copy_from_slice(&points[candidate][..dim]);
    }

    let mut assignments = vec![0usize; points.len()];
    let mut sums = vec![0f32; k * dim];
    let mut counts = vec![0usize; k];
    for _ in 0..iterations.max(1) {
        let mut changed = false;
        for (point_index, point) in points.iter().enumerate() {
            let mut best = 0usize;
            let mut best_distance = f32::INFINITY;
            for centroid in 0..k {
                let base = centroid * dim;
                let distance = l2(point, &centroids[base..base + dim]);
                if distance < best_distance {
                    best_distance = distance;
                    best = centroid;
                }
            }
            if assignments[point_index] != best {
                assignments[point_index] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
        sums.fill(0.0);
        counts.fill(0);
        for (point_index, point) in points.iter().enumerate() {
            let cluster = assignments[point_index];
            counts[cluster] += 1;
            let base = cluster * dim;
            for (slot, value) in sums[base..base + dim].iter_mut().zip(point.iter()) {
                *slot += value;
            }
        }
        for centroid in 0..k {
            if counts[centroid] == 0 {
                // Re-siembra un cluster vacio con un punto real.
                let replacement = rng.below(points.len());
                centroids[centroid * dim..(centroid + 1) * dim]
                    .copy_from_slice(&points[replacement][..dim]);
                continue;
            }
            let base = centroid * dim;
            let count = counts[centroid] as f32;
            for slot in &mut centroids[base..base + dim] {
                *slot /= count;
            }
        }
        // Recalcula sumas desde cero en la siguiente iteracion.
    }
    centroids
}

#[cfg(test)]
mod tests {
    use super::IvfPqIndex;

    /// RNG determinista para corpus sinteticos.
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn normal(&mut self) -> f32 {
            let u1 = ((self.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64;
            let u2 = ((self.next_u64() >> 11) as f64) / (1u64 << 53) as f64;
            ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
        }

        fn vector(&mut self, dim: usize) -> Vec<f32> {
            (0..dim).map(|_| self.normal()).collect()
        }
    }

    /// Corpus agrupado (mezcla de gaussianas), realista para embeddings.
    fn clustered_corpus(n: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut rng = Rng(seed | 1);
        let centers: Vec<Vec<f32>> = (0..8).map(|_| rng.vector(dim)).collect();
        (0..n)
            .map(|index| {
                let center = &centers[index % centers.len()];
                center
                    .iter()
                    .map(|coordinate| coordinate + 0.25 * rng.normal())
                    .collect()
            })
            .collect()
    }

    fn l2(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
    }

    fn corpus(n: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut rng = Rng(seed | 1);
        (0..n).map(|_| rng.vector(dim)).collect()
    }

    /// AC-0125-01: el codigo tiene un byte por subespacio.
    #[test]
    // @spec AC-0125-01
    fn test_ac_0125_01_ivf_pq_quantize() {
        let index = IvfPqIndex::new(4, 2);
        let code = index.quantize(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(code.len(), 4);
        let mut index = IvfPqIndex::new(4, 2);
        index.add(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(index.len(), 1);
        assert!(index.is_trained());
    }

    /// Recall@10 >= 0.90 con PQ + re-rank exacto y compresion real.
    #[test]
    fn recall_at_10_meets_target() {
        let dim = 32;
        let vectors = clustered_corpus(2000, dim, 0x1234);
        let mut index = IvfPqIndex::new(4, 8);
        // 256 centroides por subespacio (el maximo del codigo de 1 byte).
        index.train(&vectors, 256, 32, 5).expect("entrenamiento");
        assert!(index.is_trained());
        assert_eq!(index.len(), vectors.len());

        let queries = corpus(20, dim, 0xABCD);
        let mut hits = 0usize;
        let mut total = 0usize;
        for query in &queries {
            let mut exact: Vec<(usize, f32)> = vectors
                .iter()
                .enumerate()
                .map(|(id, vector)| (id, l2(query, vector)))
                .collect();
            exact.sort_by(|a, b| a.1.total_cmp(&b.1));
            let truth: Vec<usize> = exact.iter().take(10).map(|(id, _)| *id).collect();

            // nprobe = todos los clusters: aisla la calidad de PQ + re-rank.
            let found = index.search(query, 10, 32);
            let ids: Vec<usize> = found.iter().map(|(id, _)| *id).collect();
            hits += ids.iter().filter(|id| truth.contains(id)).count();
            total += truth.len();
        }
        let recall = hits as f64 / total as f64;
        assert!(recall >= 0.90, "recall@10 = {recall}");

        // Compresion: S bytes/vector frente a 4*dim.
        let (codes, exact_bytes) = index.memory_report();
        assert!(codes < exact_bytes / 4, "codes={codes} exact={exact_bytes}");
    }

    /// La busqueda explora menos vectores que un scan completo (sublineal).
    #[test]
    fn search_is_sublinear_with_nprobe() {
        let dim = 16;
        let vectors = corpus(400, dim, 0x77);
        let mut index = IvfPqIndex::new(4, 4);
        index.train(&vectors, 8, 16, 5).expect("train");
        let query = corpus(1, dim, 0x99).remove(0);
        let (hits, stats) = index.search_with_stats(&query, 5, 2);
        assert!(!hits.is_empty());
        assert_eq!(stats.probes, 2);
        assert!(
            stats.scored < vectors.len(),
            "nprobe=2 debe puntuar menos que n: {}",
            stats.scored
        );
    }

    /// Configuraciones invalidas fallan con error accionable.
    #[test]
    fn training_validates_configuration() {
        let mut index = IvfPqIndex::new(4, 8);
        assert!(index.train(&[], 8, 4, 3).is_err());
        assert!(index.train(&[vec![0.0; 32]], 0, 4, 3).is_err());
        assert!(index.train(&[vec![0.0; 31]], 8, 4, 3).is_err());
        assert!(index.train(&[vec![0.0; 32]], 300, 4, 3).is_err());
        assert!(index.search(&[0.0; 32], 5, 2).is_empty());
    }
}

#[cfg(test)]
mod calibration_tests {
    use super::{IvfPqIndex, l2};

    struct Rng(u64);
    impl Rng {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn normal(&mut self) -> f32 {
            let u1 = ((self.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64;
            let u2 = ((self.next_u64() >> 11) as f64) / (1u64 << 53) as f64;
            ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
        }
        fn vector(&mut self, dim: usize) -> Vec<f32> {
            (0..dim).map(|_| self.normal()).collect()
        }
    }

    fn corpus(n: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut rng = Rng(seed | 1);
        (0..n).map(|_| rng.vector(dim)).collect()
    }

    /// La calibracion elige una configuracion que cumple recall y trabajo.
    #[test]
    fn calibration_meets_recall_and_work_budget() {
        let dim = 16;
        let vectors = corpus(400, dim, 0x55);
        let mut index = IvfPqIndex::new(4, 4);
        index.train(&vectors, 32, 16, 5).expect("train");
        let queries = corpus(10, dim, 0x66);
        let truth: Vec<Vec<usize>> = queries
            .iter()
            .map(|query| {
                let mut exact: Vec<(usize, f32)> = vectors
                    .iter()
                    .enumerate()
                    .map(|(id, vector)| (id, l2(query, vector)))
                    .collect();
                exact.sort_by(|a, b| a.1.total_cmp(&b.1));
                exact.iter().take(10).map(|(id, _)| *id).collect()
            })
            .collect();

        let calibration = index
            .calibrate(&queries, &truth, 10, 0.90, 400)
            .expect("hay configuracion valida");
        assert!(calibration.recall >= 0.90, "recall {}", calibration.recall);
        assert!(calibration.mean_scored <= 400.0);
        assert!(calibration.nprobe <= 16);
        assert!((1..=50).contains(&calibration.rerank_factor));

        // Objetivo imposible -> None (no miente).
        assert!(index.calibrate(&queries, &truth, 10, 1.1, 400).is_none());
        assert!(index.calibrate(&[], &[], 10, 0.9, 10).is_none());
    }
}
