//! Aceleracion SIMD (SPEC-0150): operaciones vectoriales con auto-vectorizacion.
//!
//! Los bucles sobre slices permiten que LLVM los vectorice en release. No hay
//! intrinsics ni `unsafe`; el contrato es el resultado numerico.

/// Operaciones vectoriales basicas.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimdOps;

impl SimdOps {
    /// Producto punto.
    ///
    /// Args:
    ///     a: Primer vector.
    ///     b: Segundo vector.
    ///
    /// Returns:
    ///     Suma de productos (longitud comun).
    pub fn dot_product(a: &[f64], b: &[f64]) -> f64 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    /// Similitud coseno en `-1..=1` (0 si algun vector es nulo).
    ///
    /// Args:
    ///     a: Primer vector.
    ///     b: Segundo vector.
    ///
    /// Returns:
    ///     Coseno del angulo.
    pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
        let dot = Self::dot_product(a, b);
        let norm_a = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let norm_b = b.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    /// Distancia L2.
    ///
    /// Args:
    ///     a: Primer vector.
    ///     b: Segundo vector.
    ///
    /// Returns:
    ///     Raiz de la suma de diferencias al cuadrado.
    pub fn l2_distance(a: &[f64], b: &[f64]) -> f64 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f64>()
            .sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::SimdOps;

    #[test] // @spec AC-0150-01
    fn test_ac_0150_01_dot_product() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        assert!((SimdOps::dot_product(&a, &b) - 32.0).abs() < 1e-9);
        assert!(SimdOps::cosine_similarity(&a, &b) > 0.9);
        assert!((SimdOps::l2_distance(&a, &b) - 27.0f64.sqrt()).abs() < 1e-9);
        assert_eq!(SimdOps::cosine_similarity(&[0.0], &[1.0]), 0.0);
    }
}
