#![allow(missing_docs)]
#![allow(dead_code)]
//! IVF-PQ para vectores grandes (SPEC-0125): cuantización por subespacios.

/// Índice IVF-PQ (stub: cuantización por subespacios).
#[derive(Debug, Clone)]
pub struct IvfPqIndex {
    /// Número de subespacios.
    num_subspaces: usize,
    /// Dimensión por subespacio.
    subspace_dim: usize,
    /// Código books (cuantizados).
    codes: Vec<Vec<u8>>,
}

impl IvfPqIndex {
    /// Crea un índice IVF-PQ.
    pub fn new(num_subspaces: usize, subspace_dim: usize) -> Self {
        Self {
            num_subspaces,
            subspace_dim,
            codes: Vec::new(),
        }
    }

    /// Cuantiza un vector (reduce dimensión por factor).
    pub fn quantize(&self, vector: &[f32]) -> Vec<u8> {
        let mut code = Vec::with_capacity(self.num_subspaces);
        for i in 0..self.num_subspaces {
            let start = i * self.subspace_dim;
            let end = (start + self.subspace_dim).min(vector.len());
            let subvec = &vector[start..end];
            // Cuantización simple: promedio del subespacio → byte
            let avg: f32 = if subvec.is_empty() {
                0.0
            } else {
                subvec.iter().sum::<f32>() / subvec.len() as f32
            };
            code.push((avg * 127.0 + 128.0).clamp(0.0, 255.0) as u8);
        }
        code
    }

    /// Añade un vector cuantizado al índice.
    pub fn add(&mut self, vector: &[f32]) {
        self.codes.push(self.quantize(vector));
    }

    /// Número de vectores indexados.
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// Indica si está vacío.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// Número de subespacios.
    pub fn num_subspaces(&self) -> usize {
        self.num_subspaces
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0125-01
    fn test_ac_0125_01_ivf_pq_quantize() {
        let index = IvfPqIndex::new(4, 2);
        let vector = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let code = index.quantize(&vector);
        assert_eq!(code.len(), 4);
        let mut idx = IvfPqIndex::new(4, 2);
        idx.add(&vector);
        assert_eq!(idx.len(), 1);
    }
}
