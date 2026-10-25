#![allow(missing_docs)]
#![allow(dead_code)]
//! Fuzzy scoring para FTS (SPEC-0123): BM25 + Levenshtein.

/// Scorer fuzzy que combina BM25 con distancia de edición.
#[derive(Debug, Clone, Copy, Default)]
pub struct FuzzyScorer;

impl FuzzyScorer {
    /// Calcula un score fuzzy combinado.
    ///
    /// `bm25_score`: score BM25 del documento.
    /// `query_term`: término de búsqueda.
    /// `doc_term`: término del documento.
    /// `fuzzy_weight`: peso del componente fuzzy (0.0 a 1.0).
    pub fn score(bm25_score: f64, query_term: &str, doc_term: &str, fuzzy_weight: f64) -> f64 {
        let distance = levenshtein(query_term, doc_term) as f64;
        let max_len = query_term.len().max(doc_term.len()) as f64;
        let similarity = if max_len == 0.0 {
            1.0
        } else {
            1.0 - distance / max_len
        };
        let fuzzy_component = similarity * fuzzy_weight;
        let bm25_component = bm25_score * (1.0 - fuzzy_weight);
        bm25_component + fuzzy_component
    }

    /// Ranking de documentos por score fuzzy (mayor a menor).
    pub fn rank(
        documents: &[(f64, &str, &str)],
        query: &str,
        fuzzy_weight: f64,
    ) -> Vec<(usize, f64)> {
        let mut scored: Vec<(usize, f64)> = documents
            .iter()
            .enumerate()
            .map(|(i, (bm25, doc_term, _))| (i, Self::score(*bm25, query, doc_term, fuzzy_weight)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }
}

/// Distancia de Levenshtein (reutilizada de fts_advanced).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0123-01
    fn test_ac_0123_01_fuzzy_score() {
        // Documento exacto: score alto
        let exact = FuzzyScorer::score(1.0, "hello", "hello", 0.5);
        // Documento fuzzy: score menor
        let fuzzy = FuzzyScorer::score(1.0, "hello", "hallo", 0.5);
        assert!(exact > fuzzy, "documento exacto debe tener score mayor");
    }

    #[test]
    fn test_fuzzy_rank() {
        let docs = vec![
            (1.0, "hello", "d1"),
            (0.5, "hallo", "d2"),
            (0.8, "world", "d3"),
        ];
        let ranking = FuzzyScorer::rank(&docs, "hello", 0.3);
        assert_eq!(ranking[0].0, 0, "documento exacto debe ser primero");
    }
}
