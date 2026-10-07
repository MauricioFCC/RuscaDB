//! Parser recursive-descent de RQL sobre los tokens del lexer.

use crate::ast::{
    CompareOp, Explain, Expr, KnnClause, Projection, Select, Statement, TraverseClause,
};
use crate::lexer::{Keyword, Spanned, Token, tokenize};
use ruscadb_core::RuscaError;

/// Prefijo común de los mensajes de error de sintaxis.
const EXPECTED_PREFIX: &str = "se esperaba ";
/// Mensaje para el entero de `LIMIT`.
const LIMIT_MESSAGE: &str = "se esperaba un entero no negativo tras LIMIT";
/// Mensaje para el entero `k` de `KNN`.
const KNN_MESSAGE: &str = "se esperaba un entero no negativo como k en KNN";
/// Mensaje para el entero `DEPTH` de `TRAVERSE`.
const DEPTH_MESSAGE: &str = "se esperaba un entero no negativo como DEPTH";

/// Analiza una sentencia RQL (`SELECT` o `EXPLAIN <select>`).
///
/// Args:
///     input: Texto de la sentencia.
///
/// Returns:
///     El [`Statement`] correspondiente.
///
/// Errors:
///     [`RuscaError::ParseError`] ante entrada inválida (con posición).
pub fn parse_statement(input: &str) -> Result<Statement, RuscaError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens, input.len());
    let statement = parser.parse_statement()?;
    parser.expect_end()?;
    Ok(statement)
}

/// Analiza el texto de una consulta RQL como un único `SELECT`.
///
/// Delega en [`parse_statement`] y desempaqueta la variante [`Statement::Select`].
/// Un `EXPLAIN` no es un `SELECT`: se rechaza con [`RuscaError::ParseError`]
/// (posición 0) para no perder la envoltura en silencio; usa [`parse_statement`]
/// si necesitas analizar `EXPLAIN`.
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
    match parse_statement(input)? {
        Statement::Select(select) => Ok(select),
        Statement::Explain(_) => Err(crate::lexer::parse_error(
            "EXPLAIN no es un SELECT; usa parse_statement",
            0,
        )),
    }
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
    fn error(&self, message: impl Into<String>) -> RuscaError {
        crate::lexer::parse_error(message, self.current_position())
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
            Err(self.error(format!("{EXPECTED_PREFIX}{}", keyword.as_str())))
        }
    }

    /// Exige un token de puntuación concreto.
    fn expect_token(&mut self, token: &Token, message: &str) -> Result<(), RuscaError> {
        if self.peek() == Some(token) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    /// Exige un identificador y lo devuelve.
    fn expect_ident(&mut self) -> Result<String, RuscaError> {
        match self.peek().cloned() {
            Some(Token::Ident(name)) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.error(format!("{EXPECTED_PREFIX}un identificador"))),
        }
    }

    /// Parsea una sentencia de nivel superior (`SELECT` o `EXPLAIN <select>`).
    fn parse_statement(&mut self) -> Result<Statement, RuscaError> {
        if self.match_keyword(Keyword::Explain) {
            let inner = self.parse_select()?;
            Ok(Statement::Explain(Explain {
                inner: Box::new(inner),
            }))
        } else {
            Ok(Statement::Select(self.parse_select()?))
        }
    }

    /// Parsea una sentencia `SELECT` completa (cláusulas en orden canónico).
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
        let knn = if self.match_keyword(Keyword::Knn) {
            Some(self.parse_knn()?)
        } else {
            None
        };
        let traverse = if self.match_keyword(Keyword::Traverse) {
            Some(self.parse_traverse()?)
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
            knn,
            traverse,
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

    /// Parsea el filtro `WHERE` (predicados unidos por `AND`, asociativo izq.).
    fn parse_filter(&mut self) -> Result<Expr, RuscaError> {
        let mut expr = self.parse_predicate()?;
        while self.match_keyword(Keyword::And) {
            let right = self.parse_predicate()?;
            expr = Expr::And(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    /// Parsea un predicado: `MATCH(...)` o una comparación escalar.
    fn parse_predicate(&mut self) -> Result<Expr, RuscaError> {
        if self.match_keyword(Keyword::Match) {
            self.parse_match()
        } else {
            self.parse_comparison()
        }
    }

    /// Parsea la búsqueda full-text `MATCH(columna, 'texto')`.
    fn parse_match(&mut self) -> Result<Expr, RuscaError> {
        self.expect_token(&Token::LParen, &format!("{EXPECTED_PREFIX}'(' tras MATCH"))?;
        let column = self.expect_ident()?;
        self.expect_token(
            &Token::Comma,
            &format!("{EXPECTED_PREFIX}',' entre la columna y el texto de MATCH"),
        )?;
        let query = self.expect_text()?;
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar MATCH"),
        )?;
        Ok(Expr::Match { column, query })
    }

    /// Exige un literal de texto entre comillas simples y lo devuelve.
    fn expect_text(&mut self) -> Result<String, RuscaError> {
        match self.peek().cloned() {
            Some(Token::Text(value)) => {
                self.advance();
                Ok(value)
            }
            _ => Err(self.error(format!("{EXPECTED_PREFIX}un texto entre comillas simples"))),
        }
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
            _ => return Err(self.error(format!("{EXPECTED_PREFIX}un operador de comparación"))),
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
            _ => return Err(self.error(format!("{EXPECTED_PREFIX}una columna o un literal"))),
        };
        self.advance();
        Ok(expr)
    }

    /// Parsea la cláusula `KNN <col> <|k|> [v1, v2, ...]`.
    fn parse_knn(&mut self) -> Result<KnnClause, RuscaError> {
        let column = self.expect_ident()?;
        self.expect_token(
            &Token::KnnOpen,
            &format!("{EXPECTED_PREFIX}'<|' tras la columna de KNN"),
        )?;
        let k = self.parse_unsigned::<u64>(KNN_MESSAGE)?;
        self.expect_token(
            &Token::KnnClose,
            &format!("{EXPECTED_PREFIX}'|>' tras k en KNN"),
        )?;
        let query = self.parse_vector()?;
        Ok(KnnClause { column, k, query })
    }

    /// Parsea la cláusula `TRAVERSE <col> DEPTH <n>`.
    fn parse_traverse(&mut self) -> Result<TraverseClause, RuscaError> {
        let column = self.expect_ident()?;
        self.expect_keyword(Keyword::Depth)?;
        let depth = self.parse_unsigned::<u16>(DEPTH_MESSAGE)?;
        Ok(TraverseClause { column, depth })
    }

    /// Parsea el vector `[v1, v2, ...]` de `KNN` (puede ser vacío).
    fn parse_vector(&mut self) -> Result<Vec<f64>, RuscaError> {
        self.expect_token(
            &Token::LBracket,
            &format!("{EXPECTED_PREFIX}'[' para el vector de KNN"),
        )?;
        let mut values = Vec::new();
        if self.peek() == Some(&Token::RBracket) {
            self.advance();
            return Ok(values);
        }
        loop {
            values.push(self.parse_vector_value()?);
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect_token(
            &Token::RBracket,
            &format!("{EXPECTED_PREFIX}']' para cerrar el vector de KNN"),
        )?;
        Ok(values)
    }

    /// Parsea un elemento numérico del vector de `KNN`.
    fn parse_vector_value(&mut self) -> Result<f64, RuscaError> {
        match self.peek().cloned() {
            Some(Token::Float(value)) => {
                self.advance();
                Ok(value)
            }
            Some(Token::Int(value)) => {
                self.advance();
                Ok(value as f64)
            }
            _ => Err(self.error(format!("{EXPECTED_PREFIX}un número en el vector de KNN"))),
        }
    }

    /// Parsea un entero no negativo convirtiéndolo al tipo destino.
    fn parse_unsigned<T>(&mut self, message: &str) -> Result<T, RuscaError>
    where
        T: TryFrom<i64>,
    {
        match self.peek().cloned() {
            Some(Token::Int(value)) => {
                let number = T::try_from(value).map_err(|_| self.error(message))?;
                self.advance();
                Ok(number)
            }
            _ => Err(self.error(message)),
        }
    }

    /// Parsea el entero no negativo de `LIMIT`.
    fn parse_limit(&mut self) -> Result<u64, RuscaError> {
        self.parse_unsigned::<u64>(LIMIT_MESSAGE)
    }

    /// Verifica que no sobren tokens.
    fn expect_end(&mut self) -> Result<(), RuscaError> {
        if self.position == self.tokens.len() {
            Ok(())
        } else {
            Err(self.error("tokens sobrantes tras la consulta"))
        }
    }
}
