//! Time-series avanzado (SPEC-0118): downsampling y agregación.

/// Downsampler de series temporales.
#[derive(Debug, Clone, Copy, Default)]
pub struct Downsampler;

impl Downsampler {
    /// Reduce la resolución de una serie por promedio en ventanas de `window_size`.
    pub fn downsample(values: &[f64], window_size: usize) -> Vec<f64> {
        if window_size == 0 || values.is_empty() {
            return values.to_vec();
        }
        values
            .chunks(window_size)
            .map(|chunk| chunk.iter().sum::<f64>() / chunk.len() as f64)
            .collect()
    }

    /// Reduce por máximo en ventanas.
    pub fn downsample_max(values: &[f64], window_size: usize) -> Vec<f64> {
        if window_size == 0 || values.is_empty() {
            return values.to_vec();
        }
        values
            .chunks(window_size)
            .map(|chunk| chunk.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
            .collect()
    }

    /// Reduce por mínimo en ventanas.
    pub fn downsample_min(values: &[f64], window_size: usize) -> Vec<f64> {
        if window_size == 0 || values.is_empty() {
            return values.to_vec();
        }
        values
            .chunks(window_size)
            .map(|chunk| chunk.iter().cloned().fold(f64::INFINITY, f64::min))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0118-01
    fn test_ac_0118_01_downsample() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let down = Downsampler::downsample(&values, 2);
        assert_eq!(down.len(), 3);
        assert!((down[0] - 1.5).abs() < 1e-9);
        assert!((down[1] - 3.5).abs() < 1e-9);
        assert!((down[2] - 5.5).abs() < 1e-9);
    }

    #[test]
    fn test_downsample_max_min() {
        let values = vec![1.0, 4.0, 2.0, 3.0];
        assert_eq!(Downsampler::downsample_max(&values, 2), vec![4.0, 3.0]);
        assert_eq!(Downsampler::downsample_min(&values, 2), vec![1.0, 2.0]);
    }
}
