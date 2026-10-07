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
    /// `KNN`
    Knn,
    /// `TRAVERSE`
    Traverse,
    /// `DEPTH`
    Depth,
    /// `EXPLAIN`
    Explain,
    /// `MATCH` (búsqueda full-text dentro del `WHERE`).
    Match,
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
            Self::Knn => "KNN",
            Self::Traverse => "TRAVERSE",
            Self::Depth => "DEPTH",
            Self::Explain => "EXPLAIN",
            Self::Match => "MATCH",
        }
    }

    /// Reconoce una palabra clave a partir de su forma en minúsculas.
    ///
    /// Args:
    ///     word: Palabra en minúsculas.
    ///
    /// Returns:
    ///     La palabra clave, o `None` si no es reservada.
    pub fn from_lowercase(word: &str) -> Option<Self> {
        match word {
            "select" => Some(Self::Select),
            "from" => Some(Self::From),
            "where" => Some(Self::Where),
            "and" => Some(Self::And),
            "limit" => Some(Self::Limit),
            "knn" => Some(Self::Knn),
            "traverse" => Some(Self::Traverse),
            "depth" => Some(Self::Depth),
            "explain" => Some(Self::Explain),
            "match" => Some(Self::Match),
            _ => None,
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
    /// `<|` (apertura de la vecindad KNN).
    KnnOpen,
    /// `|>` (cierre de la vecindad KNN).
    KnnClose,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `(`
    LParen,
    /// `)`
    RParen,
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
pub(crate) fn parse_error(message: impl Into<String>, position: usize) -> RuscaError {
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
///     [`RuscaError::ParseError`] ante un carácter, número o literal inválido.
pub fn tokenize(input: &str) -> Result<Vec<Spanned>, RuscaError> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(spanned) = lexer.next_token()? {
        tokens.push(spanned);
    }
    Ok(tokens)
}

/// Estado del lexer: texto y cursor de bytes.
struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Lexer<'a> {
    /// Crea un lexer sobre el texto dado.
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            index: 0,
        }
    }

    /// Devuelve el siguiente token, o `None` al agotar la entrada.
    fn next_token(&mut self) -> Result<Option<Spanned>, RuscaError> {
        while self.index < self.bytes.len() && self.bytes[self.index].is_ascii_whitespace() {
            self.index += 1;
        }
        if self.index >= self.bytes.len() {
            return Ok(None);
        }
        let position = self.index;
        let token = match self.bytes[self.index] {
            b'*' => self.single(Token::Star),
            b',' => self.single(Token::Comma),
            b'=' => self.single(Token::Eq),
            b'!' if self.peek_next() == Some(b'=') => {
                self.index += 2;
                Token::NotEq
            }
            b'!' => return Err(parse_error("se esperaba '!='", position)),
            b'<' => self.consume_lt(),
            b'>' => self.consume_optional_eq(Token::GtEq, Token::Gt),
            b'|' if self.peek_next() == Some(b'>') => {
                self.index += 2;
                Token::KnnClose
            }
            b'|' => return Err(parse_error("se esperaba '|>'", position)),
            b'[' => self.single(Token::LBracket),
            b']' => self.single(Token::RBracket),
            b'(' => self.single(Token::LParen),
            b')' => self.single(Token::RParen),
            b'\'' => Token::Text(self.read_string(position)?),
            b'0'..=b'9' => self.read_number(position)?,
            byte if byte.is_ascii_alphabetic() || byte == b'_' => self.read_word(),
            other => {
                return Err(parse_error(
                    format!("carácter inesperado '{}'", other as char),
                    position,
                ));
            }
        };
        Ok(Some(Spanned::new(token, position)))
    }

    /// Byte siguiente al cursor, si existe.
    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.index + 1).copied()
    }

    /// Consume un token de un byte y avanza.
    fn single(&mut self, token: Token) -> Token {
        self.index += 1;
        token
    }

    /// Consume `<|` (KNN), `<=` o `<` según el byte siguiente.
    fn consume_lt(&mut self) -> Token {
        match self.peek_next() {
            Some(b'|') => {
                self.index += 2;
                Token::KnnOpen
            }
            Some(b'=') => {
                self.index += 2;
                Token::LtEq
            }
            _ => {
                self.index += 1;
                Token::Lt
            }
        }
    }

    /// Consume `<=`/`>=` si el siguiente byte es `=`, o `<`/`>` si no.
    fn consume_optional_eq(&mut self, with_eq: Token, without: Token) -> Token {
        if self.peek_next() == Some(b'=') {
            self.index += 2;
            with_eq
        } else {
            self.index += 1;
            without
        }
    }

    /// Lee un identificador o palabra clave.
    fn read_word(&mut self) -> Token {
        let start = self.index;
        while self.index < self.bytes.len()
            && (self.bytes[self.index].is_ascii_alphanumeric() || self.bytes[self.index] == b'_')
        {
            self.index += 1;
        }
        let word = &self.input[start..self.index];
        Keyword::from_lowercase(&word.to_ascii_lowercase())
            .map_or_else(|| Token::Ident(word.to_string()), Token::Keyword)
    }

    /// Lee un número entero o flotante, rechazando overflow y no-finitos.
    fn read_number(&mut self, position: usize) -> Result<Token, RuscaError> {
        let start = self.index;
        let mut is_float = false;
        while self.index < self.bytes.len() {
            match self.bytes[self.index] {
                b'0'..=b'9' => self.index += 1,
                b'.' if !is_float => {
                    is_float = true;
                    self.index += 1;
                }
                _ => break,
            }
        }
        let text = &self.input[start..self.index];
        if is_float {
            let value: f64 = text
                .parse()
                .map_err(|_| parse_error("número flotante inválido", position))?;
            if !value.is_finite() {
                return Err(parse_error("número flotante no finito", position));
            }
            Ok(Token::Float(value))
        } else {
            let value: i64 = text
                .parse()
                .map_err(|_| parse_error("entero fuera de rango", position))?;
            Ok(Token::Int(value))
        }
    }

    /// Lee un literal de texto entre comillas simples.
    fn read_string(&mut self, position: usize) -> Result<String, RuscaError> {
        self.index += 1;
        let start = self.index;
        while self.index < self.bytes.len() && self.bytes[self.index] != b'\'' {
            self.index += 1;
        }
        if self.index >= self.bytes.len() {
            return Err(parse_error("cadena sin cerrar", position));
        }
        let text = self.input[start..self.index].to_string();
        self.index += 1;
        Ok(text)
    }
}
