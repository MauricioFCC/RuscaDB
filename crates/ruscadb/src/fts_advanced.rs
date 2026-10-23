//! Full-text search avanzado (SPEC-0117): fuzzy + phrase matching.

/// Matcher fuzzy basado en distancia de Levenshtein.
#[derive(Debug, Clone, Copy, Default)]
pub struct FuzzyMatcher;

impl FuzzyMatcher {
    /// Calcula la distancia de Levenshtein entre dos cadenas.
    pub fn levenshtein(a: &str, b: &str) -> usize {
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

    /// Indica si dos cadenas están dentro de una distancia máxima.
    pub fn is_fuzzy_match(a: &str, b: &str, max_distance: usize) -> bool {
        Self::levenshtein(a, b) <= max_distance
    }
}

/// Matcher de frases (términos en orden).
#[derive(Debug, Clone, Copy, Default)]
pub struct PhraseMatcher;

impl PhraseMatcher {
    /// Verifica si `phrase` aparece como subsecuencia en `text`.
    pub fn match_phrase(text: &str, phrase: &str) -> bool {
        let text_words: Vec<&str> = text.split_whitespace().collect();
        let phrase_words: Vec<&str> = phrase.split_whitespace().collect();
        if phrase_words.is_empty() {
            return true;
        }
        let mut ti = 0;
        for pw in &phrase_words {
            while ti < text_words.len() && text_words[ti] != *pw {
                ti += 1;
            }
            if ti >= text_words.len() {
                return false;
            }
            ti += 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0117-01
    fn test_ac_0117_01_levenshtein() {
        assert_eq!(FuzzyMatcher::levenshtein("kitten", "sitting"), 3);
        assert_eq!(FuzzyMatcher::levenshtein("", "abc"), 3);
        assert_eq!(FuzzyMatcher::levenshtein("abc", "abc"), 0);
        assert!(FuzzyMatcher::is_fuzzy_match("hello", "hallo", 1));
    }

    #[test] // @spec AC-0117-02
    fn test_ac_0117_02_phrase_match() {
        assert!(PhraseMatcher::match_phrase(
            "the quick brown fox",
            "quick fox"
        ));
        assert!(!PhraseMatcher::match_phrase(
            "the quick brown fox",
            "fox quick"
        ));
        assert!(PhraseMatcher::match_phrase("hello world", ""));
    }
}
