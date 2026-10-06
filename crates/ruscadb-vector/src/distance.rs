//! Distancias vectoriales para el índice ANN.

use ruscadb_core::{Metric, RuscaError};

/// Distancia entre dos vectores según la métrica (menor = más cercano).
///
/// Args:
///     metric: Métrica de distancia.
///     a: Primer vector.
///     b: Segundo vector (misma dimensión).
///
/// Returns:
///     La distancia (L2 euclídea, `1 - coseno`, o `-producto interno`).
///
/// Errors:
///     [`RuscaError::DimensionMismatch`] si las dimensiones difieren.
pub fn distance(metric: Metric, a: &[f32], b: &[f32]) -> Result<f32, RuscaError> {
    if a.len() != b.len() {
        return Err(RuscaError::DimensionMismatch {
            expected: a.len(),
            actual: b.len(),
        });
    }
    Ok(raw_distance(metric, a, b))
}

/// Distancia sin validar dimensiones (uso interno; `a` y `b` ya son iguales).
pub(crate) fn raw_distance(metric: Metric, a: &[f32], b: &[f32]) -> f32 {
    match metric {
        Metric::L2 => a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt(),
        Metric::InnerProduct => -a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>(),
        Metric::Cosine => cosine_distance(a, b),
    }
}

/// Distancia coseno (`1 - similitud`); 1.0 si algún vector es nulo.
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        1.0
    } else {
        1.0 - dot / (norm_a * norm_b)
    }
}
