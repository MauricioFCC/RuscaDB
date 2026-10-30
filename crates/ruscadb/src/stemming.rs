//! Stemming (SPEC-0144): algoritmo de Porter (1980), base de Snowball/Porter2.
//!
//! Implementa las cinco fases clasicas de Porter sobre ASCII (minusculas):
//! `sses/ies/s`, `eed/ed/ing`, `y -> i`, y las eliminaciones por medida `m`
//! de las fases 2-5. Es determinista y `O(n)` por palabra.
//!
//! Referencias: M. F. Porter, "An algorithm for suffix stripping", Program
//! 14(3), 1980; Snowball English (Porter2) — <https://snowballstem.org/>.

/// Stemmer de Porter.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnowballStemmer;

impl SnowballStemmer {
    /// Reduce una palabra a su raiz.
    ///
    /// Args:
    ///     word: Palabra original (se pasa a minusculas).
    ///
    /// Returns:
    ///     Raiz stemizada.
    pub fn stem(word: &str) -> String {
        let lowered = word.to_lowercase();
        let bytes: Vec<u8> = lowered
            .bytes()
            .filter(|byte| byte.is_ascii_alphabetic())
            .collect();
        if bytes.len() <= 2 {
            return String::from_utf8_lossy(&bytes).into_owned();
        }
        let stemmed = porter(&bytes);
        String::from_utf8_lossy(&stemmed).into_owned()
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
            .map(Self::stem)
            .filter(|token| !token.is_empty())
            .collect()
    }
}

/// `true` si `word[i]` es consonante (la `y` depende de la anterior).
fn is_consonant(word: &[u8], index: usize) -> bool {
    match word[index] {
        b'a' | b'e' | b'i' | b'o' | b'u' => false,
        b'y' => index == 0 || !is_consonant(word, index - 1),
        _ => true,
    }
}

/// Medida `m`: numero de secuencias vocal-consonante.
fn measure(word: &[u8]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while index < word.len() && is_consonant(word, index) {
        index += 1;
    }
    while index < word.len() {
        while index < word.len() && !is_consonant(word, index) {
            index += 1;
        }
        if index >= word.len() {
            break;
        }
        count += 1;
        while index < word.len() && is_consonant(word, index) {
            index += 1;
        }
    }
    count
}

/// `true` si la palabra contiene alguna vocal.
fn contains_vowel(word: &[u8]) -> bool {
    (0..word.len()).any(|index| !is_consonant(word, index))
}

/// `true` si termina en `suffix`.
fn ends_with(word: &[u8], suffix: &[u8]) -> bool {
    word.ends_with(suffix)
}

/// `true` si termina con consonante doble.
fn ends_with_double_consonant(word: &[u8]) -> bool {
    let len = word.len();
    len >= 2 && word[len - 1] == word[len - 2] && is_consonant(word, len - 1)
}

/// Patron cvc (consonante-vocal-consonante), con ultima no `w`, `x`, `y`.
fn ends_with_cvc(word: &[u8]) -> bool {
    let len = word.len();
    if len < 3 {
        return false;
    }
    let last = word[len - 1];
    is_consonant(word, len - 3)
        && !is_consonant(word, len - 2)
        && is_consonant(word, len - 1)
        && !matches!(last, b'w' | b'x' | b'y')
}

/// Sustituye el sufijo `suffix` por `replacement` si `m > min_measure`.
fn replace_if_measure(
    word: &mut Vec<u8>,
    suffix: &[u8],
    replacement: &[u8],
    min_measure: usize,
) -> bool {
    if !ends_with(word, suffix) {
        return false;
    }
    let stem_len = word.len() - suffix.len();
    if measure(&word[..stem_len]) <= min_measure {
        return false;
    }
    word.truncate(stem_len);
    word.extend_from_slice(replacement);
    true
}

/// Aplica el algoritmo de Porter.
fn porter(input: &[u8]) -> Vec<u8> {
    let mut word = input.to_vec();
    step_1a(&mut word);
    step_1b(&mut word);
    step_1c(&mut word);
    step_2(&mut word);
    step_3(&mut word);
    step_4(&mut word);
    step_5a(&mut word);
    step_5b(&mut word);
    word
}

/// Fase 1a: plurales.
fn step_1a(word: &mut Vec<u8>) {
    if ends_with(word, b"sses") || ends_with(word, b"ies") {
        // `sses -> ss` e `ies -> i` recortan dos bytes.
        word.truncate(word.len() - 2);
    } else if ends_with(word, b"ss") {
        // sin cambios
    } else if ends_with(word, b"s") {
        word.truncate(word.len() - 1);
    }
}

/// Fase 1b: `eed`, `ed`, `ing`.
fn step_1b(word: &mut Vec<u8>) {
    if ends_with(word, b"eed") {
        let stem_len = word.len() - 3;
        if measure(&word[..stem_len]) > 0 {
            word.truncate(stem_len);
            word.push(b'e');
        }
        return;
    }
    let removed = if ends_with(word, b"ed") {
        Some(2)
    } else if ends_with(word, b"ing") {
        Some(3)
    } else {
        None
    };
    if let Some(suffix_len) = removed {
        let stem_len = word.len() - suffix_len;
        if contains_vowel(&word[..stem_len]) {
            word.truncate(stem_len);
            if ends_with(word, b"at") || ends_with(word, b"bl") || ends_with(word, b"iz") {
                word.push(b'e');
            } else if ends_with_double_consonant(word)
                && !matches!(word.last().copied(), Some(b'l' | b's' | b'z'))
            {
                word.pop();
            } else if measure(word) == 1 && ends_with_cvc(word) {
                word.push(b'e');
            }
        }
    }
}

/// Fase 1c: `y` final -> `i` si la raiz tiene vocal.
fn step_1c(word: &mut [u8]) {
    if word.last() == Some(&b'y') {
        let stem_len = word.len() - 1;
        if contains_vowel(&word[..stem_len]) {
            word[stem_len] = b'i';
        }
    }
}

/// Fase 2: sufijos derivativos (mapeo).
fn step_2(word: &mut Vec<u8>) {
    const MAPPINGS: &[(&[u8], &[u8])] = &[
        (b"ational", b"ate"),
        (b"tional", b"tion"),
        (b"enci", b"ence"),
        (b"anci", b"ance"),
        (b"izer", b"ize"),
        (b"bli", b"ble"),
        (b"alli", b"al"),
        (b"entli", b"ent"),
        (b"eli", b"e"),
        (b"ousli", b"ous"),
        (b"ization", b"ize"),
        (b"ation", b"ate"),
        (b"ator", b"ate"),
        (b"alism", b"al"),
        (b"iveness", b"ive"),
        (b"fulness", b"ful"),
        (b"ousness", b"ous"),
        (b"aliti", b"al"),
        (b"iviti", b"ive"),
        (b"biliti", b"ble"),
        (b"logi", b"log"),
    ];
    for (suffix, replacement) in MAPPINGS {
        if replace_if_measure(word, suffix, replacement, 0) {
            return;
        }
    }
}

/// Fase 3: sufijos derivativos (eliminacion).
fn step_3(word: &mut Vec<u8>) {
    const MAPPINGS: &[(&[u8], &[u8])] = &[
        (b"icate", b"ic"),
        (b"ative", b""),
        (b"alize", b"al"),
        (b"iciti", b"ic"),
        (b"ical", b"ic"),
        (b"ful", b""),
        (b"ness", b""),
    ];
    for (suffix, replacement) in MAPPINGS {
        if replace_if_measure(word, suffix, replacement, 0) {
            return;
        }
    }
}

/// Fase 4: eliminacion de sufijos con `m > 1`.
fn step_4(word: &mut Vec<u8>) {
    const SUFFIXES: &[&[u8]] = &[
        b"al", b"ance", b"ence", b"er", b"ic", b"able", b"ible", b"ant", b"ement", b"ment", b"ent",
        b"ou", b"ism", b"ate", b"iti", b"ous", b"ive", b"ize",
    ];
    for suffix in SUFFIXES {
        if replace_if_measure(word, suffix, b"", 1) {
            return;
        }
    }
    // `ion` solo si la raiz acaba en `s` o `t`.
    if ends_with(word, b"ion") {
        let stem_len = word.len() - 3;
        if measure(&word[..stem_len]) > 1 && matches!(word[stem_len - 1], b's' | b't') {
            word.truncate(stem_len);
        }
    }
}

/// Fase 5a: `e` final.
fn step_5a(word: &mut Vec<u8>) {
    if !ends_with(word, b"e") {
        return;
    }
    let stem_len = word.len() - 1;
    let stem_measure = measure(&word[..stem_len]);
    if stem_measure > 1 || (stem_measure == 1 && !ends_with_cvc(&word[..stem_len])) {
        word.pop();
    }
}

/// Fase 5b: `ll` final -> `l` si `m > 1`.
fn step_5b(word: &mut Vec<u8>) {
    if ends_with(word, b"ll") && measure(word) > 1 {
        word.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::SnowballStemmer;

    #[test] // @spec AC-0144-01
    fn test_ac_0144_01_stem_english() {
        // Vectores canonicos del paper de Porter (1980).
        let cases = [
            ("caresses", "caress"),
            ("ponies", "poni"),
            ("ties", "ti"),
            ("caress", "caress"),
            ("cats", "cat"),
            ("feed", "feed"),
            ("plastered", "plaster"),
            ("bled", "bled"),
            ("motoring", "motor"),
            ("sing", "sing"),
            ("conflated", "conflat"),
            ("troubled", "troubl"),
            ("sized", "size"),
            ("hopping", "hop"),
            ("tanned", "tan"),
            ("falling", "fall"),
            ("hissing", "hiss"),
            ("fizzed", "fizz"),
            ("failing", "fail"),
            ("filing", "file"),
            ("happy", "happi"),
            ("sky", "sky"),
        ];
        for (word, expected) in cases {
            assert_eq!(SnowballStemmer::stem(word), expected, "stem({word})");
        }
        let tokens = SnowballStemmer::tokenize("The runners are running");
        assert!(tokens.contains(&"run".to_string()), "{tokens:?}");
        assert!(tokens.contains(&"runner".to_string()) || tokens.contains(&"run".to_string()));
    }

    /// Fases 2-5: derivativos y medidas.
    #[test]
    fn derivational_suffixes() {
        let cases = [
            ("relational", "relat"),
            ("conditional", "condit"),
            ("rational", "ration"),
            ("digitizer", "digit"),
            ("vietnamization", "vietnam"),
            ("predication", "predic"),
            ("operator", "oper"),
            ("feudalism", "feudal"),
            ("decisiveness", "decis"),
            ("hopefulness", "hope"),
            ("callousness", "callous"),
            ("formaliti", "formal"),
            ("sensitiviti", "sensit"),
            ("triplicate", "triplic"),
            ("formative", "form"),
            ("formalize", "formal"),
            ("electriciti", "electr"),
            ("electrical", "electr"),
            ("hopeful", "hope"),
            ("goodness", "good"),
            ("revival", "reviv"),
            ("allowance", "allow"),
            ("inference", "infer"),
            ("airliner", "airlin"),
            ("adjustable", "adjust"),
            ("defensible", "defens"),
            ("irritant", "irrit"),
            ("replacement", "replac"),
            ("adjustment", "adjust"),
            ("dependent", "depend"),
            ("adoption", "adopt"),
            ("communism", "commun"),
            ("activate", "activ"),
            ("effective", "effect"),
            ("bowdlerize", "bowdler"),
            ("probate", "probat"),
            ("rate", "rate"),
            ("cease", "ceas"),
            ("controll", "control"),
            ("roll", "roll"),
        ];
        for (word, expected) in cases {
            assert_eq!(SnowballStemmer::stem(word), expected, "stem({word})");
        }
    }

    /// Palabras cortas, no ASCII y vacias no rompen.
    #[test]
    fn edge_cases() {
        assert_eq!(SnowballStemmer::stem(""), "");
        assert_eq!(SnowballStemmer::stem("a"), "a");
        assert_eq!(SnowballStemmer::stem("is"), "is");
        assert_eq!(SnowballStemmer::stem("123"), "");
        assert_eq!(SnowballStemmer::stem("Running!"), "run");
    }
}
