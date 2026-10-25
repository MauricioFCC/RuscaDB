//! Stemming Snowball (SPEC-0144): stub con stemming inglés.

/// Stemmer Snowball (stub: inglés).
#[derive(Debug, Clone, Copy, Default)]
pub struct SnowballStemmer;

impl SnowballStemmer {
    /// Aplica stemming a una palabra (inglés).
    pub fn stem(word: &str) -> String {
        let w = word.to_lowercase();
        if w.len() <= 3 {
            return w;
        }
        for suffix in &["ing", "tion", "ed", "er", "est", "ly", "ment", "ness", "s"] {
            if w.ends_with(suffix) && w.len() > suffix.len() + 2 {
                return w[..w.len() - suffix.len()].to_string();
            }
        }
        w
    }

    /// Tokeniza y stemmea un texto.
    pub fn tokenize(text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|w| {
                let cleaned: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
                Self::stem(&cleaned)
            })
            .filter(|w| !w.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0144-01
    fn test_ac_0144_01_stem_english() {
        assert_eq!(SnowballStemmer::stem("running"), "runn");
        assert_eq!(SnowballStemmer::stem("playing"), "play");
        let tokens = SnowballStemmer::tokenize("The runners are running");
        assert!(!tokens.is_empty());
    }
}
