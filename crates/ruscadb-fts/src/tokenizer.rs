//! Tokenizador Unicode propio, sin dependencias externas.
//!
//! El tokenizador es deliberadamente simple y **auditable**: segmenta por
//! límites de palabra Unicode, normaliza a minúsculas y no aplica stemming,
//! sinónimos ni listas de stopwords. Su determinismo es un requisito de
//! ranking (NF-0014-03).

/// Tokeniza `text` en términos normalizados.
///
/// Segmenta la entrada por límites de palabra Unicode usando
/// [`char::is_alphanumeric`] (todo carácter no alfanumérico actúa como
/// separador), convierte cada segmento a minúsculas con [`str::to_lowercase`] y
/// descarta los segmentos vacíos. Es un tokenizador propio (sin dependencias
/// externas de tokenización) para que el comportamiento sea auditable y
/// determinista: no elimina stopwords ni aplica stemming.
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
        if character.is_alphanumeric() {
            current.push(character);
        } else if !current.is_empty() {
            push_lowercased(&mut tokens, &current);
            current.clear();
        }
    }
    if !current.is_empty() {
        push_lowercased(&mut tokens, &current);
    }
    tokens
}

/// Añade la versión en minúsculas de `current` a `tokens` y reinicia el buffer.
///
/// Args:
///     tokens: Acumulador de términos ya normalizados.
///     current: Segmento alfanumérico en su forma original.
fn push_lowercased(tokens: &mut Vec<String>, current: &str) {
    tokens.push(current.to_lowercase());
}
