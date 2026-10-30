//! Busqueda por proximidad (SPEC-0152): phrase queries con `slop`.
//!
//! Comprueba que los terminos de una frase aparezcan en orden y separados por
//! como mucho `slop` palabras intermedias.

/// Matcher de proximidad para frases.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProximityMatcher;

impl ProximityMatcher {
    /// Comprueba si `phrase` aparece en `text` con `slop` de tolerancia.
    ///
    /// Args:
    ///     text: Texto donde buscar.
    ///     phrase: Frase buscada (terminos separados por espacios).
    ///     slop: Palabras intermedias permitidas entre terminos consecutivos.
    ///
    /// Returns:
    ///     `true` si todos los terminos aparecen en orden dentro del `slop`.
    pub fn match_proximity(text: &str, phrase: &str, slop: usize) -> bool {
        let text_words: Vec<&str> = text.split_whitespace().collect();
        let phrase_words: Vec<&str> = phrase.split_whitespace().collect();
        if phrase_words.is_empty() {
            return true;
        }
        if phrase_words.len() == 1 {
            return text_words.contains(&phrase_words[0]);
        }
        let mut positions: Vec<usize> = Vec::new();
        for word in &phrase_words {
            if let Some(position) = text_words.iter().position(|candidate| candidate == word) {
                positions.push(position);
            } else {
                return false;
            }
        }
        for window in positions.windows(2) {
            if window[1] <= window[0] || window[1] - window[0] > slop + 1 {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::ProximityMatcher;

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
