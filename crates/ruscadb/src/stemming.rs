//! Stemming Snowball (SPEC-0144): stemmer ingles.
//!
//! Reglas de sufijos para ingles (subset de Snowball). No es el algoritmo
//! completo: cubre los sufijos mas frecuentes y es determinista.

/// Stemmer Snowball (ingles).
#[derive(Debug, Clone, Copy, Default)]
pub struct SnowballStemmer;

impl SnowballStemmer {
    /// Reduce una palabra a su raiz.
    ///
    /// Args:
    ///     word: Palabra original.
    ///
    /// Returns:
    ///     Raiz en minusculas (la palabra si es corta o no tiene sufijo).
    pub fn stem(word: &str) -> String {
        let lowered = word.to_lowercase();
        if lowered.len() <= 3 {
            return lowered;
        }
        for suffix in &["ing", "tion", "ed", "er", "est", "ly", "ment", "ness", "s"] {
            if lowered.ends_with(suffix) && lowered.len() > suffix.len() + 2 {
                return lowered[..lowered.len() - suffix.len()].to_string();
            }
        }
        lowered
    }

    /// Tokeniza y aplica stemming.
    ///
    /// Args:
    ///     text: Texto libre.
    ///
    /// Returns:
    ///     Raices no vacias en orden.
    pub fn tokenize(text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|word| {
                let cleaned: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
                Self::stem(&cleaned)
            })
            .filter(|word| !word.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::SnowballStemmer;

    #[test] // @spec AC-0144-01
    fn test_ac_0144_01_stem_english() {
        assert_eq!(SnowballStemmer::stem("running"), "runn");
        assert_eq!(SnowballStemmer::stem("playing"), "play");
        let tokens = SnowballStemmer::tokenize("The runners are running");
        assert!(!tokens.is_empty());
    }
}
