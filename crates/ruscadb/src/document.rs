//! Operadores documentales `->` y `@>` (SPEC-0044).
//!
//! La fachada evalúa `Record.doc` (un `serde_json::Value`) sin declarar
//! `serde_json` en su `Cargo.toml`: la extracción `->` usa los métodos
//! inherentes del valor y la contención `@>` serializa el documento y lo compara
//! contra el literal JSON parseado por este lector propio. El lector implementa
//! el subconjunto de JSON que emite `serde_json::Value::to_string()` más los
//! literales habituales de las consultas (`null`, booleanos, números, textos,
//! arreglos y objetos).

use std::collections::BTreeMap;

use ruscadb_core::{Record, RuscaError, ScalarValue};

/// Valor JSON mínimo para evaluar la contención `@>`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonValue {
    /// `null`.
    Null,
    /// `true`/`false`.
    Bool(bool),
    /// Entero.
    Int(i64),
    /// Flotante.
    Float(f64),
    /// Texto.
    Text(String),
    /// Arreglo ordenado.
    Array(Vec<JsonValue>),
    /// Objeto con claves ordenadas (comparación determinista).
    Object(BTreeMap<String, JsonValue>),
}

/// Extrae el campo de la ruta `path` del documento del registro (SPEC-0044).
///
/// Args:
///     record: Registro cuyo `doc` se consulta.
///     path: Ruta de claves anidadas (`["a"]`, `["a", "b"]`).
///
/// Returns:
///     El escalar extraído, o `None` si falta el documento o el campo, o si el
///     valor no es escalar (arreglo u objeto). La ausencia excluye la fila sin
///     error.
pub(crate) fn extract_doc_scalar(record: &Record, path: &[String]) -> Option<ScalarValue> {
    let mut current = record.doc.as_ref()?;
    for key in path {
        current = current.get(key.as_str())?;
    }
    if current.is_null() {
        return Some(ScalarValue::Null);
    }
    if let Some(flag) = current.as_bool() {
        return Some(ScalarValue::Bool(flag));
    }
    if let Some(number) = current.as_i64() {
        return Some(ScalarValue::Int(number));
    }
    if let Some(number) = current.as_f64() {
        return Some(ScalarValue::Float(number));
    }
    current
        .as_str()
        .map(|text| ScalarValue::Text(text.to_string()))
}

/// Comprueba si el documento del registro contiene `json` (SPEC-0044).
///
/// Args:
///     record: Registro cuyo `doc` se consulta.
///     json: Subdocumento esperado como texto JSON.
///
/// Returns:
///     `true` si el documento contiene el subdocumento; `false` si el registro
///     no tiene documento o no lo contiene.
///
/// Errors:
///     [`RuscaError::ParseError`] si `json` no es JSON válido (error accionable).
pub(crate) fn doc_contains(record: &Record, json: &str) -> Result<bool, RuscaError> {
    let expected = parse_json(json).map_err(|message| RuscaError::ParseError {
        message: format!("JSON inválido en el operador '@>': {message}"),
        position: 0,
    })?;
    let Some(document) = record.doc.as_ref() else {
        return Ok(false);
    };
    let serialized = document.to_string();
    let container = parse_json(&serialized).map_err(|message| {
        RuscaError::CorruptManifest(format!(
            "el documento del registro no es JSON válido: {message}"
        ))
    })?;
    Ok(contains(&container, &expected))
}

/// Comprueba la contención `@>` entre dos valores JSON.
///
/// Semántica estilo PostgreSQL: un objeto contiene a otro si toda clave del
/// esperado existe en el contenedor con un valor contenido; un arreglo contiene
/// a otro si cada elemento esperado está contenido en algún elemento del
/// contenedor; un escalar solo se contiene a sí mismo (con igualdad numérica
/// entre enteros y flotantes).
///
/// Args:
///     container: Valor JSON del documento.
///     expected: Subdocumento esperado.
///
/// Returns:
///     `true` si `container` contiene a `expected`.
fn contains(container: &JsonValue, expected: &JsonValue) -> bool {
    match expected {
        JsonValue::Array(items) => match container {
            JsonValue::Array(values) => items
                .iter()
                .all(|item| values.iter().any(|value| contains(value, item))),
            _ => false,
        },
        JsonValue::Object(fields) => match container {
            JsonValue::Object(values) => fields
                .iter()
                .all(|(key, value)| values.get(key).is_some_and(|inner| contains(inner, value))),
            _ => false,
        },
        scalar => scalar_equals(container, scalar),
    }
}

/// Igualdad de escalares JSON (enteros y flotantes se comparan numéricamente).
///
/// Args:
///     left: Primer escalar.
///     right: Segundo escalar.
///
/// Returns:
///     `true` si ambos representan el mismo valor.
fn scalar_equals(left: &JsonValue, right: &JsonValue) -> bool {
    match (left, right) {
        (JsonValue::Null, JsonValue::Null) => true,
        (JsonValue::Bool(first), JsonValue::Bool(second)) => first == second,
        (JsonValue::Text(first), JsonValue::Text(second)) => first == second,
        (JsonValue::Int(first), JsonValue::Int(second)) => first == second,
        (JsonValue::Float(first), JsonValue::Float(second)) => first == second,
        (JsonValue::Int(integer), JsonValue::Float(number))
        | (JsonValue::Float(number), JsonValue::Int(integer)) => (*integer as f64) == *number,
        _ => false,
    }
}

/// Estado del lector JSON: texto y cursor de bytes.
struct JsonParser<'a> {
    /// Texto JSON de entrada.
    input: &'a str,
    /// Cursor de bytes.
    index: usize,
}

/// Parsea un texto JSON completo al [`JsonValue`] mínimo.
///
/// Args:
///     input: Texto JSON.
///
/// Returns:
///     El valor parseado.
///
/// Errors:
///     Mensaje accionable (con posición) si el JSON es inválido o sobra texto.
fn parse_json(input: &str) -> Result<JsonValue, String> {
    let mut parser = JsonParser { input, index: 0 };
    parser.skip_whitespace();
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.index != input.len() {
        return Err(format!(
            "contenido inesperado tras el JSON (posición {})",
            parser.index
        ));
    }
    Ok(value)
}

impl JsonParser<'_> {
    /// Avanza sobre espacios en blanco JSON.
    fn skip_whitespace(&mut self) {
        while matches!(self.peek_byte(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.index += 1;
        }
    }

    /// Byte actual, si queda alguno.
    fn peek_byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.index).copied()
    }

    /// Carácter actual (UTF-8), si queda alguno.
    fn peek_char(&self) -> Option<char> {
        self.input[self.index..].chars().next()
    }

    /// Exige un byte concreto y avanza.
    fn expect_byte(&mut self, expected: u8) -> Result<(), String> {
        if self.peek_byte() == Some(expected) {
            self.index += 1;
            Ok(())
        } else {
            Err(format!(
                "se esperaba '{}' en la posición {}",
                expected as char, self.index
            ))
        }
    }

    /// Parsea un valor JSON cualquiera.
    fn parse_value(&mut self) -> Result<JsonValue, String> {
        match self.peek_byte() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(JsonValue::Text(self.parse_string()?)),
            Some(b't' | b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            Some(other) => Err(format!(
                "carácter inesperado '{}' en JSON (posición {})",
                other as char, self.index
            )),
            None => Err("JSON vacío o truncado".to_string()),
        }
    }

    /// Parsea un objeto `{ "clave": valor, ... }`.
    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect_byte(b'{')?;
        let mut fields = BTreeMap::new();
        self.skip_whitespace();
        if self.peek_byte() == Some(b'}') {
            self.index += 1;
            return Ok(JsonValue::Object(fields));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect_byte(b':')?;
            self.skip_whitespace();
            fields.insert(key, self.parse_value()?);
            self.skip_whitespace();
            match self.peek_byte() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    return Ok(JsonValue::Object(fields));
                }
                _ => return Err(format!("se esperaba ',' o '}}' (posición {})", self.index)),
            }
        }
    }

    /// Parsea un arreglo `[ valor, ... ]`.
    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect_byte(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek_byte() == Some(b']') {
            self.index += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek_byte() {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(format!("se esperaba ',' o ']' (posición {})", self.index)),
            }
        }
    }

    /// Parsea `true` o `false`.
    fn parse_bool(&mut self) -> Result<JsonValue, String> {
        if self.consume_literal("true") {
            Ok(JsonValue::Bool(true))
        } else if self.consume_literal("false") {
            Ok(JsonValue::Bool(false))
        } else {
            Err(format!(
                "literal booleano inválido (posición {})",
                self.index
            ))
        }
    }

    /// Parsea `null`.
    fn parse_null(&mut self) -> Result<JsonValue, String> {
        if self.consume_literal("null") {
            Ok(JsonValue::Null)
        } else {
            Err(format!("literal nulo inválido (posición {})", self.index))
        }
    }

    /// Consume un literal textual (`true`/`false`/`null`) si coincide.
    fn consume_literal(&mut self, literal: &str) -> bool {
        if self.input[self.index..].starts_with(literal) {
            self.index += literal.len();
            true
        } else {
            false
        }
    }

    /// Parsea un número entero o flotante.
    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.index;
        while matches!(
            self.peek_byte(),
            Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
        ) {
            self.index += 1;
        }
        let text = &self.input[start..self.index];
        if !text.contains(['.', 'e', 'E']) {
            if let Ok(integer) = text.parse::<i64>() {
                return Ok(JsonValue::Int(integer));
            }
        }
        text.parse::<f64>()
            .map(JsonValue::Float)
            .map_err(|_| format!("número JSON inválido '{text}' (posición {start})"))
    }

    /// Parsea una cadena entre comillas con escapes.
    fn parse_string(&mut self) -> Result<String, String> {
        self.expect_byte(b'"')?;
        let mut text = String::new();
        loop {
            let Some(character) = self.peek_char() else {
                return Err("cadena JSON sin cerrar".to_string());
            };
            match character {
                '"' => {
                    self.index += 1;
                    return Ok(text);
                }
                '\\' => {
                    self.index += 1;
                    text.push(self.parse_escape()?);
                }
                control if (control as u32) < 0x20 => {
                    return Err(format!(
                        "carácter de control sin escapar (posición {})",
                        self.index
                    ));
                }
                other => {
                    text.push(other);
                    self.index += other.len_utf8();
                }
            }
        }
    }

    /// Parsea la secuencia de escape tras una barra invertida.
    fn parse_escape(&mut self) -> Result<char, String> {
        let Some(character) = self.peek_char() else {
            return Err("escape JSON incompleto".to_string());
        };
        self.index += character.len_utf8();
        match character {
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            '/' => Ok('/'),
            'b' => Ok('\u{0008}'),
            'f' => Ok('\u{000C}'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'u' => self.parse_unicode_escape(),
            other => Err(format!("escape JSON inválido '\\{other}'")),
        }
    }

    /// Parsea un escape `\uXXXX` (cuatro dígitos hexadecimales).
    fn parse_unicode_escape(&mut self) -> Result<char, String> {
        let end = self.index + 4;
        let Some(hex) = self.input.get(self.index..end) else {
            return Err("escape \\u incompleto".to_string());
        };
        let code =
            u32::from_str_radix(hex, 16).map_err(|_| format!("escape \\u inválido '{hex}'"))?;
        self.index = end;
        char::from_u32(code).ok_or_else(|| format!("código \\u inválido: {code}"))
    }
}
