//! Tokenizador Unicode propio, sin dependencias externas.
//!
//! El tokenizador es deliberadamente simple y **auditable**: segmenta por
//! límites de palabra Unicode, normaliza a minúsculas y no aplica stemming,
//! sinónimos ni listas de stopwords. Su determinismo es un requisito de
//! ranking (NF-0014-03) y es **idempotente**: `tokenize(token)` vuelve a dar
//! el mismo token (regresión del proptest del CI, 2026-10-12).

/// Tokeniza `text` en términos normalizados.
///
/// Segmenta por [`char::is_alphanumeric`] **después** de aplicar el lowercase
/// Unicode (para que el mapeo sea estable: `"İ"` → `"i"`) y descarta los
/// diacríticos combinantes (Mn), que no deben romper ni formar tokens. Es un
/// tokenizador propio (sin dependencias externas) para que el comportamiento
/// sea auditable y determinista: no elimina stopwords ni aplica stemming.
///
/// Args:
///     text: Texto UTF-8 de entrada (puede ser vacío).
///
/// Returns:
///     Términos en minúsculas en orden de aparición; nunca contiene cadenas
///     vacías.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        // El lowercase primero (puede expandir a varios caracteres, p. ej.
        // "İ" → "i" + U+0307) garantiza la idempotencia de la tokenización.
        for lowered in character.to_lowercase() {
            if is_combining_mark(lowered) {
                continue; // Los diacríticos combinantes no separan ni forman token.
            }
            if lowered.is_alphanumeric() {
                current.push(lowered);
            } else if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Indica si un carácter es un diacrítico combinante (categoría Unicode Mn).
///
/// Cubre los bloques Mn habituales sin dependencias externas.
///
/// Args:
///     character: Carácter a clasificar.
///
/// Returns:
///     `true` si pertenece a un bloque de marcas no espaciadas.
fn is_combining_mark(character: char) -> bool {
    matches!(
        character as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// Regresión del proptest del CI: la tokenización es idempotente ante el
    /// lowercase Unicode (`"İ"` → `"i"`), de modo que un término indexado se
    /// recupera al re-tokenizarlo.
    #[test]
    fn tokenize_is_idempotent_for_unicode_lowercase() {
        let tokens = tokenize("İ");
        assert_eq!(tokens, vec!["i".to_string()], "İ debe normalizar a i");
        assert_eq!(tokenize(&tokens[0]), tokens, "tokenize(token) == token");
        assert_eq!(tokenize("I"), tokens, "I y İ coinciden");
        assert_eq!(
            tokenize("i\u{0307}"),
            tokens,
            "el combining no rompe el token"
        );
    }

    /// La segmentación básica y el lowercase se mantienen.
    #[test]
    fn tokenize_splits_and_lowercases() {
        assert_eq!(
            tokenize("Gato, PERRO!"),
            vec!["gato".to_string(), "perro".to_string()]
        );
        assert!(tokenize("   ,,,   ").is_empty());
    }
}
