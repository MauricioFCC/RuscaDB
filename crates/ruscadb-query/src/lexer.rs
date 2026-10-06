//! Lexer de RQL: convierte texto en una secuencia de tokens con posición.

use ruscadb_core::RuscaError;

/// Palabra clave reservada de RQL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    /// `SELECT`
    Select,
    /// `FROM`
    From,
    /// `WHERE`
    Where,
    /// `AND`
    And,
    /// `LIMIT`
    Limit,
}

impl Keyword {
    /// Texto canónico de la palabra clave.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Select => "SELECT",
            Self::From => "FROM",
            Self::Where => "WHERE",
            Self::And => "AND",
            Self::Limit => "LIMIT",
        }
    }
}

/// Token léxico de RQL.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// Palabra clave.
    Keyword(Keyword),
    /// Identificador.
    Ident(String),
    /// Entero.
    Int(i64),
    /// Flotante.
    Float(f64),
    /// Texto entre comillas simples.
    Text(String),
    /// `*`
    Star,
    /// `,`
    Comma,
    /// `=`
    Eq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    LtEq,
    /// `>`
    Gt,
    /// `>=`
    GtEq,
}

/// Token con su posición (byte) en el texto original.
#[derive(Clone, Debug, PartialEq)]
pub struct Spanned {
    /// Token.
    pub token: Token,
    /// Posición inicial en bytes.
    pub position: usize,
}

impl Spanned {
    fn new(token: Token, position: usize) -> Self {
        Self { token, position }
    }
}

/// Construye un [`RuscaError::ParseError`] con posición.
pub fn parse_error(message: impl Into<String>, position: usize) -> RuscaError {
    RuscaError::ParseError {
        message: message.into(),
        position,
    }
}

/// Tokeniza el texto de una consulta RQL.
///
/// Args:
///     input: Texto de la consulta.
///
/// Returns:
///     La lista de tokens con posición.
///
/// Errors:
///     [`RuscaError::ParseError`] ante un carácter o literal inválido.
pub fn tokenize(input: &str) -> Result<Vec<Spanned>, RuscaError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let position = index;
        match byte {
            b'*' => push(&mut tokens, Token::Star, position, &mut index, 1),
            b',' => push(&mut tokens, Token::Comma, position, &mut index, 1),
            b'=' => push(&mut tokens, Token::Eq, position, &mut index, 1),
            b'!' if bytes.get(index + 1) == Some(&b'=') => {
                push(&mut tokens, Token::NotEq, position, &mut index, 2);
            }
            b'!' => return Err(parse_error("se esperaba '!='", position)),
            b'<' if bytes.get(index + 1) == Some(&b'=') => {
                push(&mut tokens, Token::LtEq, position, &mut index, 2);
            }
            b'<' => push(&mut tokens, Token::Lt, position, &mut index, 1),
            b'>' if bytes.get(index + 1) == Some(&b'=') => {
                push(&mut tokens, Token::GtEq, position, &mut index, 2);
            }
            b'>' => push(&mut tokens, Token::Gt, position, &mut index, 1),
            b'\'' => {
                let (text, next) = read_string(input, index)?;
                tokens.push(Spanned::new(Token::Text(text), position));
                index = next;
            }
            b'0'..=b'9' => {
                let (token, next) = read_number(input, index);
                tokens.push(Spanned::new(token, position));
                index = next;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let (token, next) = read_word(input, index);
                tokens.push(Spanned::new(token, position));
                index = next;
            }
            other => {
                return Err(parse_error(
                    format!("carácter inesperado '{}'", other as char),
                    position,
                ));
            }
        }
    }
    Ok(tokens)
}

/// Empuja un token de longitud `width` y avanza el índice.
fn push(tokens: &mut Vec<Spanned>, token: Token, position: usize, index: &mut usize, width: usize) {
    tokens.push(Spanned::new(token, position));
    *index += width;
}

/// Lee un identificador o palabra clave.
fn read_word(input: &str, start: usize) -> (Token, usize) {
    let bytes = input.as_bytes();
    let mut end = start;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    let word = &input[start..end];
    let token = match word.to_ascii_lowercase().as_str() {
        "select" => Token::Keyword(Keyword::Select),
        "from" => Token::Keyword(Keyword::From),
        "where" => Token::Keyword(Keyword::Where),
        "and" => Token::Keyword(Keyword::And),
        "limit" => Token::Keyword(Keyword::Limit),
        _ => Token::Ident(word.to_string()),
    };
    (token, end)
}

/// Lee un número entero o flotante.
fn read_number(input: &str, start: usize) -> (Token, usize) {
    let bytes = input.as_bytes();
    let mut end = start;
    let mut is_float = false;
    while end < bytes.len() {
        match bytes[end] {
            b'0'..=b'9' => end += 1,
            b'.' if !is_float => {
                is_float = true;
                end += 1;
            }
            _ => break,
        }
    }
    let text = &input[start..end];
    let token = if is_float {
        Token::Float(text.parse().unwrap_or(0.0))
    } else {
        Token::Int(text.parse().unwrap_or(0))
    };
    (token, end)
}

/// Lee un literal de texto entre comillas simples.
fn read_string(input: &str, start: usize) -> Result<(String, usize), RuscaError> {
    let bytes = input.as_bytes();
    let mut end = start + 1;
    while end < bytes.len() && bytes[end] != b'\'' {
        end += 1;
    }
    if end >= bytes.len() {
        return Err(parse_error("cadena sin cerrar", start));
    }
    Ok((input[start + 1..end].to_string(), end + 1))
}
