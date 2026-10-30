//! Aceleracion numerica (SPEC-0150): sumas compensadas y desenrollado.
//!
//! `forbid(unsafe_code)` impide intrinsics SIMD; en su lugar se usan tecnicas
//! que el compilador puede auto-vectorizar y que mejoran la precision:
//! acumuladores independientes (rompe dependencias) y suma compensada de
//! Neumaier (error `O(eps)` en vez de `O(n*eps)`).
//!
//! Referencias: Neumaier (1974), "Rounding error analysis of some methods for
//! summing finite sums"; Kahan (1965).

/// Operaciones vectoriales.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimdOps;

impl SimdOps {
    /// Indica si se usan intrinsics SIMD de hardware.
    ///
    /// Returns:
    ///     `false`: la crate prohibe `unsafe`, el compilador auto-vectoriza.
    pub const fn is_hardware_simd() -> bool {
        false
    }

    /// Producto punto con 4 acumuladores independientes (auto-vectorizable).
    ///
    /// Args:
    ///     a: Primer vector.
    ///     b: Segundo vector.
    ///
    /// Returns:
    ///     Suma de productos (longitud comun).
    pub fn dot_product(a: &[f64], b: &[f64]) -> f64 {
        let len = a.len().min(b.len());
        let chunks = len / 4;
        let mut accumulators = [0.0f64; 4];
        for chunk in 0..chunks {
            let base = chunk * 4;
            for lane in 0..4 {
                accumulators[lane] += a[base + lane] * b[base + lane];
            }
        }
        let mut sum = accumulators[0] + accumulators[1] + accumulators[2] + accumulators[3];
        for index in chunks * 4..len {
            sum += a[index] * b[index];
        }
        sum
    }

    /// Producto punto compensado (Neumaier).
    ///
    /// Args:
    ///     a: Primer vector.
    ///     b: Segundo vector.
    ///
    /// Returns:
    ///     Suma con correccion de error de redondeo.
    pub fn dot_product_compensated(a: &[f64], b: &[f64]) -> f64 {
        let mut sum = 0.0f64;
        let mut compensation = 0.0f64;
        for (x, y) in a.iter().zip(b.iter()) {
            let product = x * y;
            let total = sum + product;
            compensation += if sum.abs() >= product.abs() {
                (sum - total) + product
            } else {
                (product - total) + sum
            };
            sum = total;
        }
        sum + compensation
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
        let dot = Self::dot_product_compensated(a, b);
        let norm_a = Self::dot_product_compensated(a, a).sqrt();
        let norm_b = Self::dot_product_compensated(b, b).sqrt();
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
        let mut sum = 0.0f64;
        let mut compensation = 0.0f64;
        for (x, y) in a.iter().zip(b.iter()) {
            let difference = x - y;
            let square = difference * difference;
            let total = sum + square;
            compensation += if sum.abs() >= square.abs() {
                (sum - total) + square
            } else {
                (square - total) + sum
            };
            sum = total;
        }
        (sum + compensation).max(0.0).sqrt()
    }

    /// Media con suma compensada.
    ///
    /// Args:
    ///     values: Valores.
    ///
    /// Returns:
    ///     Media (`0.0` si vacio).
    pub fn mean(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        let mut sum = 0.0f64;
        let mut compensation = 0.0f64;
        for value in values {
            let total = sum + value;
            compensation += if sum.abs() >= value.abs() {
                (sum - total) + value
            } else {
                (value - total) + sum
            };
            sum = total;
        }
        (sum + compensation) / values.len() as f64
    }

    /// Normaliza un vector a norma 1 (copia).
    ///
    /// Args:
    ///     values: Vector.
    ///
    /// Returns:
    ///     Vector normalizado (vacio si la norma es 0).
    pub fn normalize(values: &[f64]) -> Vec<f64> {
        let norm = Self::dot_product_compensated(values, values).sqrt();
        if norm == 0.0 {
            return Vec::new();
        }
        values.iter().map(|value| value / norm).collect()
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
        assert!(!SimdOps::is_hardware_simd());
    }

    /// Precisión: la suma compensada bate a la ingenua en cancelación catastrófica.
    #[test]
    fn compensated_summation_is_more_accurate() {
        let mut values = vec![1.0e16];
        values.extend(std::iter::repeat_n(1.0, 1_000));
        values.push(-1.0e16);
        // Neumaier conserva los 1.0; la suma ingenua los pierde.
        let compensated = SimdOps::dot_product_compensated(&values, &vec![1.0; values.len()]);
        assert!(
            (compensated - 1_000.0).abs() < 1e-6,
            "compensada: {compensated}"
        );
        let mean = SimdOps::mean(&[1.0, 2.0, 3.0, 4.0]);
        assert!((mean - 2.5).abs() < 1e-12);
        assert!(
            (SimdOps::normalize(&[3.0, 4.0])
                .iter()
                .map(|value| value * value)
                .sum::<f64>()
                - 1.0)
                .abs()
                < 1e-12
        );
        assert!(SimdOps::normalize(&[0.0]).is_empty());
    }
}
