//! Continuous aggregates para time-series (SPEC-0145): stub.

/// Agregado continuo (stub).
#[derive(Debug, Clone)]
pub struct ContinuousAggregate {
    /// Nombre del agregado.
    pub name: String,
    /// Tamaño de ventana en ms.
    pub window_ms: i64,
}

impl ContinuousAggregate {
    /// Crea un agregado continuo.
    pub fn new(name: impl Into<String>, window_ms: i64) -> Self {
        Self {
            name: name.into(),
            window_ms,
        }
    }

    /// Computa agregados por ventana (promedio).
    pub fn compute(&self, points: &[(i64, f64)]) -> Vec<(i64, f64)> {
        if self.window_ms == 0 || points.is_empty() {
            return points.to_vec();
        }
        let mut result = Vec::new();
        let mut window_start = points[0].0;
        let mut sum = 0.0;
        let mut count = 0usize;
        for &(ts, val) in points {
            if ts - window_start >= self.window_ms {
                if count > 0 {
                    result.push((window_start, sum / count as f64));
                }
                window_start = ts;
                sum = 0.0;
                count = 0;
            }
            sum += val;
            count += 1;
        }
        if count > 0 {
            result.push((window_start, sum / count as f64));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0145-01
    fn test_ac_0145_01_continuous_aggregate() {
        let agg = ContinuousAggregate::new("avg_1m", 60000);
        let points = vec![(0, 1.0), (1000, 2.0), (60000, 3.0), (61000, 4.0)];
        let result = agg.compute(&points);
        assert_eq!(result.len(), 2);
    }
}
