#![allow(missing_docs)]
#![allow(dead_code)]
//! Aceleracion SIMD (SPEC-0150): stub con auto-vectorizacion.

#[derive(Debug, Clone, Copy, Default)]
pub struct SimdOps;

impl SimdOps {
    pub fn dot_product(a: &[f64], b: &[f64]) -> f64 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }
    pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
        let dot = Self::dot_product(a, b);
        let na = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let nb = b.iter().map(|x| x * x).sum::<f64>().sqrt();
        if na == 0.0 || nb == 0.0 {
            0.0
        } else {
            dot / (na * nb)
        }
    }
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
    use super::*;
    #[test] // @spec AC-0150-01
    fn test_ac_0150_01_dot_product() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        let dot = SimdOps::dot_product(&a, &b);
        assert!((dot - 32.0).abs() < 1e-9);
        let cos = SimdOps::cosine_similarity(&a, &b);
        assert!(cos > 0.9);
    }
}
