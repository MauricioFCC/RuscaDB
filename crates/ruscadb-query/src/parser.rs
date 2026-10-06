//! Parser recursive-descent de RQL sobre los tokens del lexer.

use crate::ast::{CompareOp, Expr, Projection, Select};
use crate::lexer::{Keyword, Spanned, Token, tokenize};
use ruscadb_core::RuscaError;

/// Analiza el texto de una consulta RQL.
///
/// Args:
///     input: Texto de la consulta.
///
/// Returns:
///     El IR [`Select`] correspondiente.
///
/// Errors:
///     [`RuscaError::ParseError`] ante entrada inválida (con posición).
pub fn parse(input: &str) -> Result<Select, RuscaError> {
    parse_impl(input)
}

fn parse_impl(input: &str) -> Result<Select, RuscaError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens, input.len());
    let select = parser.parse_select()?;
    parser.expect_end()?;
    Ok(select)
}

/// Estado del parser: tokens y cursor.
struct Parser {
    tokens: Vec<Spanned>,
    position: usize,
    end: usize,
}

impl Parser {
    /// Crea un parser sobre la lista de tokens (con longitud total del texto).
    fn new(tokens: Vec<Spanned>, end: usize) -> Self {
        Self {
            tokens,
            position: 0,
            end,
        }
    }

    /// Token actual, si queda alguno.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position).map(|spanned| &spanned.token)
    }

    /// Posición del token actual (o el final del texto).
    fn current_position(&self) -> usize {
        self.tokens
            .get(self.position)
            .map_or(self.end, |spanned| spanned.position)
    }

    /// Construye un error de sintaxis en la posición actual.
    fn error<T>(&self, message: impl Into<String>) -> Result<T, RuscaError> {
        Err(crate::lexer::parse_error(message, self.current_position()))
    }

    /// Avanza el cursor.
    fn advance(&mut self) {
        self.position += 1;
    }

    /// Consume la palabra clave si coincide.
    fn match_keyword(&mut self, keyword: Keyword) -> bool {
        if self.peek() == Some(&Token::Keyword(keyword)) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Exige una palabra clave.
    fn expect_keyword(&mut self, keyword: Keyword) -> Result<(), RuscaError> {
        if self.match_keyword(keyword) {
            Ok(())
        } else {
            self.error(format!("se esperaba {}", keyword.as_str()))
        }
    }

    /// Exige un identificador y lo devuelve.
    fn expect_ident(&mut self) -> Result<String, RuscaError> {
        match self.peek().cloned() {
            Some(Token::Ident(name)) => {
                self.advance();
                Ok(name)
            }
            _ => self.error("se esperaba un identificador"),
        }
    }

    /// Parsea una sentencia `SELECT` completa.
    fn parse_select(&mut self) -> Result<Select, RuscaError> {
        self.expect_keyword(Keyword::Select)?;
        let projection = self.parse_projection()?;
        self.expect_keyword(Keyword::From)?;
        let from = self.expect_ident()?;
        let filter = if self.match_keyword(Keyword::Where) {
            Some(self.parse_filter()?)
        } else {
            None
        };
        let limit = if self.match_keyword(Keyword::Limit) {
            Some(self.parse_limit()?)
        } else {
            None
        };
        Ok(Select {
            projection,
            from,
            filter,
            limit,
        })
    }

    /// Parsea la proyección (`*` o lista de columnas).
    fn parse_projection(&mut self) -> Result<Projection, RuscaError> {
        if self.peek() == Some(&Token::Star) {
            self.advance();
            return Ok(Projection::All);
        }
        let mut columns = vec![self.expect_ident()?];
        while self.peek() == Some(&Token::Comma) {
            self.advance();
            columns.push(self.expect_ident()?);
        }
        Ok(Projection::Columns(columns))
    }

    /// Parsea el filtro `WHERE` (comparaciones unidas por `AND`, asociativo izq.).
    fn parse_filter(&mut self) -> Result<Expr, RuscaError> {
        let mut expr = self.parse_comparison()?;
        while self.match_keyword(Keyword::And) {
            let right = self.parse_comparison()?;
            expr = Expr::And(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    /// Parsea una comparación `expr op expr`.
    fn parse_comparison(&mut self) -> Result<Expr, RuscaError> {
        let left = self.parse_expr()?;
        let op = self.parse_compare_op()?;
        let right = self.parse_expr()?;
        Ok(Expr::Compare {
            left: Box::new(left),
            op,
            right: Box::new(right),
        })
    }

    /// Parsea un operador de comparación.
    fn parse_compare_op(&mut self) -> Result<CompareOp, RuscaError> {
        let op = match self.peek() {
            Some(Token::Eq) => CompareOp::Eq,
            Some(Token::NotEq) => CompareOp::NotEq,
            Some(Token::Lt) => CompareOp::Lt,
            Some(Token::LtEq) => CompareOp::LtEq,
            Some(Token::Gt) => CompareOp::Gt,
            Some(Token::GtEq) => CompareOp::GtEq,
            _ => return self.error("se esperaba un operador de comparación"),
        };
        self.advance();
        Ok(op)
    }

    /// Parsea un operando (columna o literal).
    fn parse_expr(&mut self) -> Result<Expr, RuscaError> {
        let expr = match self.peek().cloned() {
            Some(Token::Ident(name)) => Expr::Column(name),
            Some(Token::Int(value)) => Expr::Int(value),
            Some(Token::Float(value)) => Expr::Float(value),
            Some(Token::Text(value)) => Expr::Text(value),
            _ => return self.error("se esperaba una columna o un literal"),
        };
        self.advance();
        Ok(expr)
    }

    /// Parsea el entero no negativo de `LIMIT`.
    fn parse_limit(&mut self) -> Result<u64, RuscaError> {
        match self.peek().cloned() {
            Some(Token::Int(value)) if value >= 0 => {
                self.advance();
                Ok(value as u64)
            }
            _ => self.error("se esperaba un entero no negativo tras LIMIT"),
        }
    }

    /// Verifica que no sobren tokens.
    fn expect_end(&mut self) -> Result<(), RuscaError> {
        if self.position == self.tokens.len() {
            Ok(())
        } else {
            self.error("tokens sobrantes tras la consulta")
        }
    }
}
