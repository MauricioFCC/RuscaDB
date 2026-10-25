#![allow(missing_docs)]
#![allow(dead_code)]
//! Busqueda por proximidad (SPEC-0152): phrase queries con slop.

#[derive(Debug, Clone, Copy, Default)]
pub struct ProximityMatcher;

impl ProximityMatcher {
    /// Verifica si los terminos de una frase aparecen dentro de una distancia maxima (slop).
    pub fn match_proximity(text: &str, phrase: &str, slop: usize) -> bool {
        let text_words: Vec<&str> = text.split_whitespace().collect();
        let phrase_words: Vec<&str> = phrase.split_whitespace().collect();
        if phrase_words.is_empty() {
            return true;
        }
        if phrase_words.len() == 1 {
            return text_words.contains(&phrase_words[0]);
        }
        // Check if consecutive phrase words are within slop distance
        let mut positions: Vec<usize> = Vec::new();
        for pw in &phrase_words {
            if let Some(pos) = text_words.iter().position(|w| w == pw) {
                positions.push(pos);
            } else {
                return false;
            }
        }
        for i in 1..positions.len() {
            if positions[i] <= positions[i - 1] || positions[i] - positions[i - 1] > slop + 1 {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0152-01
    fn test_ac_0152_01_proximity_match() {
        assert!(ProximityMatcher::match_proximity(
            "the quick brown fox",
            "quick fox",
            1
        ));
        assert!(!ProximityMatcher::match_proximity(
            "the quick brown fox",
            "fox quick",
            1
        ));
    }
}
