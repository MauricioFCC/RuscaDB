//! Planificador basado en costos (SPEC-0142): stub.

/// Estimador de costos.
#[derive(Debug, Clone, Copy, Default)]
pub struct CostEstimator;

impl CostEstimator {
    /// Estima el costo de un scan (costo = rows * factor).
    pub fn estimate_scan(rows: usize, factor: f64) -> f64 {
        rows as f64 * factor
    }

    /// Estima el costo de un filtro.
    pub fn estimate_filter(rows: usize, selectivity: f64, factor: f64) -> f64 {
        (rows as f64 * selectivity) * factor
    }

    /// Estima el costo de un join.
    pub fn estimate_join(left_rows: usize, right_rows: usize, factor: f64) -> f64 {
        (left_rows as f64 * right_rows as f64) * factor
    }

    /// Estima el costo de un sort.
    pub fn estimate_sort(rows: usize, factor: f64) -> f64 {
        (rows as f64 * (rows as f64).log2()) * factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0142-01
    fn test_ac_0142_01_cost_estimate() {
        let small = CostEstimator::estimate_scan(100, 1.0);
        let large = CostEstimator::estimate_scan(10000, 1.0);
        assert!(large > small);
    }
}
