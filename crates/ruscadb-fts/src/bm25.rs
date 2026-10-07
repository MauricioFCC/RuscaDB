//! Parámetros y puntuación **BM25** (Best Matching 25).
//!
//! Fórmulas de `docs/RuscaDB-roadmap.md` §5.4:
//!
//! - `idf = ln(1 + (N - df + 0.5) / (df + 0.5))`
//! - `tf_norm = tf * (k1 + 1) / (tf + k1 * (1 - b + b * dl / avgdl))`
//! - `score = idf * tf_norm`

use serde::{Deserialize, Serialize};

/// Factor de saturación por defecto (`k1`) de `docs/RuscaDB-roadmap.md` §5.4.
const DEFAULT_K1: f32 = 1.2;
/// Grado de normalización por longitud por defecto (`b`) del roadmap.
const DEFAULT_B: f32 = 0.75;
/// Suavizado de IDF que evita el logaritmo de cero.
const IDF_SMOOTHING: f32 = 0.5;

/// Ponderador BM25 con parámetros de saturación (`k1`) y normalización de
/// longitud (`b`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bm25 {
    /// Factor de saturación de la frecuencia de término (`k1`).
    pub k1: f32,
    /// Grado de normalización por longitud de documento (`b`, en `0..=1`).
    pub b: f32,
}

impl Default for Bm25 {
    /// Devuelve los parámetros del roadmap: `k1 = 1.2`, `b = 0.75`.
    fn default() -> Self {
        Self {
            k1: DEFAULT_K1,
            b: DEFAULT_B,
        }
    }
}

impl Bm25 {
    /// Calcula el IDF suavizado de un término.
    ///
    /// Args:
    ///     doc_count: Número total de documentos vivos (`N`).
    ///     doc_freq: Número de documentos que contienen el término (`df`).
    ///
    /// Returns:
    ///     `ln(1 + (N - df + 0.5) / (df + 0.5))`; `0.0` si `N == 0` o
    ///     `df == 0`.
    pub fn idf(&self, doc_count: usize, doc_freq: usize) -> f32 {
        if doc_count == 0 || doc_freq == 0 {
            return 0.0;
        }
        let total = doc_count as f32;
        let frequency = doc_freq as f32;
        let ratio = (total - frequency + IDF_SMOOTHING) / (frequency + IDF_SMOOTHING);
        (1.0 + ratio).ln()
    }

    /// Normaliza la frecuencia de término por la longitud del documento.
    ///
    /// Args:
    ///     term_freq: Veces que el término aparece en el documento (`tf`).
    ///     doc_len: Longitud del documento en términos (`dl`).
    ///     avg_doc_len: Longitud media de los documentos vivos (`avgdl`).
    ///
    /// Returns:
    ///     `tf * (k1 + 1) / (tf + k1 * (1 - b + b * dl / avgdl))`; `0.0` si
    ///     `tf == 0` o `avg_doc_len <= 0`.
    pub fn tf_norm(&self, term_freq: u32, doc_len: u32, avg_doc_len: f32) -> f32 {
        if term_freq == 0 || avg_doc_len <= 0.0 {
            return 0.0;
        }
        let frequency = term_freq as f32;
        let length = doc_len as f32;
        let length_ratio = length / avg_doc_len;
        let denominator = frequency + self.k1 * (1.0 - self.b + self.b * length_ratio);
        frequency * (self.k1 + 1.0) / denominator
    }

    /// Contribución BM25 de un término a un documento.
    ///
    /// Args:
    ///     term_freq: Veces que el término aparece en el documento (`tf`).
    ///     doc_len: Longitud del documento en términos (`dl`).
    ///     avg_doc_len: Longitud media de los documentos vivos (`avgdl`).
    ///     doc_freq: Documentos que contienen el término (`df`).
    ///     doc_count: Documentos vivos del corpus (`N`).
    ///
    /// Returns:
    ///     El producto `idf(N, df) * tf_norm(tf, dl, avgdl)`.
    pub fn score(
        &self,
        term_freq: u32,
        doc_len: u32,
        avg_doc_len: f32,
        doc_freq: usize,
        doc_count: usize,
    ) -> f32 {
        self.idf(doc_count, doc_freq) * self.tf_norm(term_freq, doc_len, avg_doc_len)
    }
}
