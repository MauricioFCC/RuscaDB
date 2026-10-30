//! IVF clustering real (SPEC-0147): k-means para particionar vectores.
//!
//! Agrupa un corpus en `num_clusters` centroides con Lloyd (init determinista)
//! y expone las asignaciones y los `nprobe` clusters mas cercanos a una query.
//! Reutiliza el k-means de [`crate::ivf_pq`] (misma implementacion, sin
//! duplicar) y es `O(iters * n * k * d)`: lineal en `n`, sin `O(n^2)`.
//!
//! Referencia: FAISS IVF (k-means + `nprobe`).

use ruscadb_core::RuscaError;

use crate::ivf_pq::kmeans;

/// Clusterizador IVF.
#[derive(Debug, Clone, Default)]
pub struct IvfClusterer {
    /// Numero de clusters deseados.
    pub num_clusters: usize,
    /// Iteraciones de Lloyd.
    pub iterations: usize,
    centroids: Vec<Vec<f32>>,
    assignments: Vec<usize>,
    dimension: usize,
}

impl IvfClusterer {
    /// Crea el clusterizador.
    ///
    /// Args:
    ///     num_clusters: Clusters deseados (minimo 1).
    ///
    /// Returns:
    ///     Clusterizador sin entrenar (8 iteraciones por defecto).
    pub fn new(num_clusters: usize) -> Self {
        Self {
            num_clusters: num_clusters.max(1),
            iterations: 8,
            centroids: Vec::new(),
            assignments: Vec::new(),
            dimension: 0,
        }
    }

    /// Ajusta las iteraciones de Lloyd.
    ///
    /// Args:
    ///     iterations: Iteraciones (minimo 1).
    pub fn with_iterations(mut self, iterations: usize) -> Self {
        self.iterations = iterations.max(1);
        self
    }

    /// Entrena centroides y calcula asignaciones.
    ///
    /// Args:
    ///     vectors: Corpus (dimension consistente).
    ///
    /// Returns:
    ///     `Ok(())` con `centroids()` y `assignments()` disponibles.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el corpus esta vacio, hay
    ///     dimensiones inconsistentes o `num_clusters == 0`.
    pub fn fit(&mut self, vectors: &[Vec<f32>]) -> Result<(), RuscaError> {
        if vectors.is_empty() {
            return Err(RuscaError::InvalidConfig(
                "IVF clustering: corpus vacio".to_string(),
            ));
        }
        let dimension = vectors[0].len();
        if dimension == 0 {
            return Err(RuscaError::InvalidConfig(
                "IVF clustering: dimension 0".to_string(),
            ));
        }
        if vectors.iter().any(|vector| vector.len() != dimension) {
            return Err(RuscaError::InvalidConfig(
                "IVF clustering: dimensiones inconsistentes".to_string(),
            ));
        }
        let points: Vec<&[f32]> = vectors.iter().map(Vec::as_slice).collect();
        let flat = kmeans(
            &points,
            self.num_clusters,
            self.iterations,
            0xC2B2_AE35_3593_4D1D,
        );
        let centroids: Vec<Vec<f32>> = flat.chunks(dimension).map(<[f32]>::to_vec).collect();
        let assignments = vectors
            .iter()
            .map(|vector| nearest(&centroids, vector))
            .collect();
        self.centroids = centroids;
        self.assignments = assignments;
        self.dimension = dimension;
        Ok(())
    }

    /// Agrupa `vectors` y devuelve la asignacion a cluster.
    ///
    /// Es la version sin estado: entrena k-means en la llamada. Para reutilizar
    /// centroides usa [`Self::fit`].
    ///
    /// Args:
    ///     vectors: Corpus.
    ///
    /// Returns:
    ///     Un indice de cluster por vector (vacio si el corpus esta vacio).
    pub fn cluster(&self, vectors: &[Vec<f32>]) -> Vec<usize> {
        if vectors.is_empty() {
            return Vec::new();
        }
        let dimension = vectors[0].len();
        if dimension == 0 {
            return vec![0; vectors.len()];
        }
        let points: Vec<&[f32]> = vectors.iter().map(Vec::as_slice).collect();
        let flat = kmeans(
            &points,
            self.num_clusters,
            self.iterations,
            0xC2B2_AE35_3593_4D1D,
        );
        let centroids: Vec<Vec<f32>> = flat.chunks(dimension).map(<[f32]>::to_vec).collect();
        vectors
            .iter()
            .map(|vector| nearest(&centroids, vector))
            .collect()
    }

    /// Centroides entrenados.
    ///
    /// Returns:
    ///     Centroides (vacio si no se llamo a `fit`).
    pub fn centroids(&self) -> &[Vec<f32>] {
        &self.centroids
    }

    /// Asignaciones del ultimo `fit`.
    ///
    /// Returns:
    ///     Cluster de cada vector (vacio si no se llamo a `fit`).
    pub fn assignments(&self) -> &[usize] {
        &self.assignments
    }

    /// Indica si el clusterizador esta entrenado.
    ///
    /// Returns:
    ///     `true` tras un `fit` correcto.
    pub fn is_fitted(&self) -> bool {
        !self.centroids.is_empty()
    }

    /// Clusters mas cercanos a `query` (para `nprobe`).
    ///
    /// Args:
    ///     query: Vector de consulta.
    ///     nprobe: Numero de clusters a devolver (>= 1).
    ///
    /// Returns:
    ///     Indices de cluster por distancia ascendente.
    pub fn nearest_clusters(&self, query: &[f32], nprobe: usize) -> Vec<usize> {
        let mut ranked: Vec<(f32, usize)> = self
            .centroids
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
    }

    /// Dimension entrenada.
    ///
    /// Returns:
    ///     Dimension del corpus de `fit` (0 si no entrenado).
    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

/// Distancia L2 al cuadrado.
fn l2(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Indice del centroide mas cercano.
fn nearest(centroids: &[Vec<f32>], vector: &[f32]) -> usize {
    let mut best = 0usize;
    let mut best_distance = f32::INFINITY;
    for (index, centroid) in centroids.iter().enumerate() {
        let distance = l2(vector, centroid);
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::IvfClusterer;

    #[test] // @spec AC-0147-01
    fn test_ac_0147_01_ivf_cluster() {
        let clusterer = IvfClusterer::new(4);
        let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
        let assignments = clusterer.cluster(&vectors);
        assert_eq!(assignments.len(), 2);
    }

    /// Dos grupos bien separados quedan en clusters distintos y `fit` los
    /// expone (centroides + asignaciones).
    #[test]
    fn fit_separates_well_separated_groups() {
        let mut vectors = Vec::new();
        for index in 0..20 {
            vectors.push(vec![10.0 + index as f32 * 0.01, 10.0]);
            vectors.push(vec![-10.0 - index as f32 * 0.01, -10.0]);
        }
        let mut clusterer = IvfClusterer::new(2).with_iterations(20);
        clusterer.fit(&vectors).expect("fit");
        assert!(clusterer.is_fitted());
        assert_eq!(clusterer.centroids().len(), 2);
        assert_eq!(clusterer.assignments().len(), vectors.len());
        assert_ne!(
            clusterer.assignments()[0],
            clusterer.assignments()[1],
            "los grupos opuestos caen en clusters distintos"
        );
        assert_eq!(clusterer.dimension(), 2);

        // nprobe: el cluster del primer vector es el mas cercano a si mismo.
        let nearest = clusterer.nearest_clusters(&vectors[0], 1);
        assert_eq!(nearest, vec![clusterer.assignments()[0]]);
    }

    /// Entradas invalidas fallan con error accionable; vacio devuelve vacio.
    #[test]
    fn validation_is_actionable() {
        let mut clusterer = IvfClusterer::new(3);
        assert!(clusterer.fit(&[]).is_err());
        assert!(clusterer.fit(&[vec![0.0, 1.0], vec![1.0]]).is_err());
        assert!(clusterer.fit(&[vec![]]).is_err());
        assert!(clusterer.cluster(&[]).is_empty());
        assert_eq!(clusterer.cluster(&[vec![]]), vec![0]);
        assert!(!IvfClusterer::new(0).is_fitted());
        assert_eq!(IvfClusterer::new(0).num_clusters, 1);
    }
}
