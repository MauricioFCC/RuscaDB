//! Parser recursive-descent de RQL sobre los tokens del lexer.

use crate::ast::{
    AggFunc, Aggregate, CompareOp, Delete, Explain, Expr, Insert, KnnClause, OrderBy, Projection,
    Select, Statement, TraverseClause, Update,
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
        Statement::Insert(_) | Statement::Update(_) | Statement::Delete(_) => Err(
            crate::lexer::parse_error("la sentencia DML no es un SELECT; usa parse_statement", 0),
        ),
    }
}

/// Comprueba que cada fila de `VALUES` tenga tantos valores como columnas.
///
/// Args:
///     columns: Columnas objetivo del `INSERT`.
///     rows: Filas de literales.
///
/// Returns:
///     `Ok(())` si todas las filas están alineadas con `columns`.
///
/// Errors:
///     Mensaje accionable si una fila tiene un número distinto de valores.
fn validate_insert_rows(columns: &[String], rows: &[Vec<Expr>]) -> Result<(), String> {
    for (index, row) in rows.iter().enumerate() {
        if row.len() != columns.len() {
            return Err(format!(
                "la fila {} de VALUES tiene {} valores, pero se esperaban {} (las columnas del INSERT)",
                index + 1,
                row.len(),
                columns.len()
            ));
        }
    }
    Ok(())
}

/// Divide una ruta documental `'a.b'` en sus segmentos `["a", "b"]`.
///
/// Args:
///     path: Ruta separada por puntos.
///
/// Returns:
///     Los segmentos de la ruta (al menos uno, ninguno vacío).
///
/// Errors:
///     Mensaje accionable si la ruta está vacía o tiene segmentos vacíos.
fn doc_path_segments(path: &str) -> Result<Vec<String>, String> {
    let segments: Vec<String> = path.split('.').map(str::to_string).collect();
    if segments.iter().any(String::is_empty) {
        return Err(format!(
            "ruta documental inválida '{path}': usa 'campo' o 'campo.anidado'"
        ));
    }
    Ok(segments)
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

    /// Parsea una sentencia de nivel superior (`SELECT`/`EXPLAIN`/DML).
    fn parse_statement(&mut self) -> Result<Statement, RuscaError> {
        match self.peek() {
            Some(Token::Keyword(Keyword::Explain)) => {
                self.advance();
                let inner = self.parse_select()?;
                Ok(Statement::Explain(Explain {
                    inner: Box::new(inner),
                }))
            }
            Some(Token::Keyword(Keyword::Insert)) => Ok(Statement::Insert(self.parse_insert()?)),
            Some(Token::Keyword(Keyword::Update)) => Ok(Statement::Update(self.parse_update()?)),
            Some(Token::Keyword(Keyword::Delete)) => Ok(Statement::Delete(self.parse_delete()?)),
            _ => Ok(Statement::Select(self.parse_select()?)),
        }
    }

    /// Parsea `INSERT INTO <tabla> (<cols>) VALUES (<fila>)[, ...]`.
    fn parse_insert(&mut self) -> Result<Insert, RuscaError> {
        self.expect_keyword(Keyword::Insert)?;
        self.expect_keyword(Keyword::Into)?;
        let table = self.expect_ident()?;
        self.expect_token(
            &Token::LParen,
            &format!("{EXPECTED_PREFIX}'(' tras la tabla"),
        )?;
        let columns = self.parse_ident_list()?;
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar la lista de columnas"),
        )?;
        self.expect_keyword(Keyword::Values)?;
        let rows = self.parse_value_rows()?;
        validate_insert_rows(&columns, &rows).map_err(|message| self.error(message))?;
        Ok(Insert {
            table,
            columns,
            rows,
        })
    }

    /// Parsea `UPDATE <tabla> SET <col> = <literal>[, ...] [WHERE ...]`.
    fn parse_update(&mut self) -> Result<Update, RuscaError> {
        self.expect_keyword(Keyword::Update)?;
        let table = self.expect_ident()?;
        self.expect_keyword(Keyword::Set)?;
        let mut assignments = Vec::new();
        loop {
            let column = self.expect_ident()?;
            self.expect_token(&Token::Eq, &format!("{EXPECTED_PREFIX}'=' en SET"))?;
            let value = self.parse_literal()?;
            assignments.push((column, value));
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let filter = if self.match_keyword(Keyword::Where) {
            Some(self.parse_filter()?)
        } else {
            None
        };
        Ok(Update {
            table,
            assignments,
            filter,
        })
    }

    /// Parsea `DELETE FROM <tabla> [WHERE ...]`.
    fn parse_delete(&mut self) -> Result<Delete, RuscaError> {
        self.expect_keyword(Keyword::Delete)?;
        self.expect_keyword(Keyword::From)?;
        let table = self.expect_ident()?;
        let filter = if self.match_keyword(Keyword::Where) {
            Some(self.parse_filter()?)
        } else {
            None
        };
        Ok(Delete { table, filter })
    }

    /// Parsea una lista `ident[, ident...]` (al menos un identificador).
    fn parse_ident_list(&mut self) -> Result<Vec<String>, RuscaError> {
        let mut names = vec![self.expect_ident()?];
        while self.peek() == Some(&Token::Comma) {
            self.advance();
            names.push(self.expect_ident()?);
        }
        Ok(names)
    }

    /// Parsea una o más filas `(<literal>, ...)[, (...)...]` de `VALUES`.
    fn parse_value_rows(&mut self) -> Result<Vec<Vec<Expr>>, RuscaError> {
        let mut rows = vec![self.parse_value_row()?];
        while self.peek() == Some(&Token::Comma) {
            self.advance();
            rows.push(self.parse_value_row()?);
        }
        Ok(rows)
    }

    /// Parsea una fila `(<literal>, ...)` de `VALUES`.
    fn parse_value_row(&mut self) -> Result<Vec<Expr>, RuscaError> {
        self.expect_token(&Token::LParen, &format!("{EXPECTED_PREFIX}'(' en VALUES"))?;
        let mut values = vec![self.parse_literal()?];
        while self.peek() == Some(&Token::Comma) {
            self.advance();
            values.push(self.parse_literal()?);
        }
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar una fila de VALUES"),
        )?;
        Ok(values)
    }

    /// Parsea un literal escalar (`entero`, `flotante` o `texto`) como [`Expr`].
    fn parse_literal(&mut self) -> Result<Expr, RuscaError> {
        let literal = match self.peek().cloned() {
            Some(Token::Int(value)) => Expr::Int(value),
            Some(Token::Float(value)) => Expr::Float(value),
            Some(Token::Text(value)) => Expr::Text(value),
            _ => {
                return Err(self.error(format!(
                    "{EXPECTED_PREFIX}un literal (entero, flotante o texto)"
                )));
            }
        };
        self.advance();
        Ok(literal)
    }

    /// Parsea una sentencia `SELECT` completa (cláusulas en orden canónico).
    fn parse_select(&mut self) -> Result<Select, RuscaError> {
        self.expect_keyword(Keyword::Select)?;
        let (projection, aggregates) = self.parse_projection()?;
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
        let group_by = if self.match_keyword(Keyword::Group) {
            self.parse_group_by()?
        } else {
            Vec::new()
        };
        let order_by = if self.match_keyword(Keyword::Order) {
            Some(self.parse_order_by()?)
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
            aggregates,
            from,
            filter,
            knn,
            traverse,
            group_by,
            order_by,
            limit,
        })
    }

    /// Parsea la proyección: `*` o una lista de columnas y agregados.
    ///
    /// Returns:
    ///     `(proyección, agregados)`; las columnas y los agregados se separan
    ///     pero conservan su orden relativo dentro de cada grupo.
    fn parse_projection(&mut self) -> Result<(Projection, Vec<Aggregate>), RuscaError> {
        if self.peek() == Some(&Token::Star) {
            self.advance();
            return Ok((Projection::All, Vec::new()));
        }
        let mut columns = Vec::new();
        let mut aggregates = Vec::new();
        loop {
            if self.is_aggregate_function() {
                aggregates.push(self.parse_aggregate()?);
            } else {
                columns.push(self.expect_ident()?);
            }
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok((Projection::Columns(columns), aggregates))
    }

    /// Indica si el token actual inicia una función de agregación.
    fn is_aggregate_function(&self) -> bool {
        matches!(
            self.peek(),
            Some(Token::Keyword(
                Keyword::Count | Keyword::Sum | Keyword::Avg | Keyword::Min | Keyword::Max
            ))
        )
    }

    /// Parsea un agregado `FUNC(<*|columna>) [AS alias]`.
    fn parse_aggregate(&mut self) -> Result<Aggregate, RuscaError> {
        let func = self.parse_agg_func()?;
        self.expect_token(
            &Token::LParen,
            &format!("{EXPECTED_PREFIX}'(' tras {}", func.as_str()),
        )?;
        let column = if self.peek() == Some(&Token::Star) {
            self.advance();
            None
        } else {
            Some(self.expect_ident()?)
        };
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar {}", func.as_str()),
        )?;
        let func = match (func, column.is_some()) {
            (AggFunc::Count, false) => AggFunc::CountStar,
            (other, false) => {
                return Err(self.error(format!(
                    "'{}(*)' no es válido: solo COUNT admite '*'; usa {}(columna)",
                    other.as_str(),
                    other.as_str()
                )));
            }
            (other, true) => other,
        };
        let alias = if self.match_keyword(Keyword::As) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(Aggregate {
            func,
            column,
            alias,
        })
    }

    /// Parsea la función de agregación y avanza el cursor.
    fn parse_agg_func(&mut self) -> Result<AggFunc, RuscaError> {
        let func = match self.peek() {
            Some(Token::Keyword(Keyword::Count)) => AggFunc::Count,
            Some(Token::Keyword(Keyword::Sum)) => AggFunc::Sum,
            Some(Token::Keyword(Keyword::Avg)) => AggFunc::Avg,
            Some(Token::Keyword(Keyword::Min)) => AggFunc::Min,
            Some(Token::Keyword(Keyword::Max)) => AggFunc::Max,
            _ => return Err(self.error(format!("{EXPECTED_PREFIX}una función de agregación"))),
        };
        self.advance();
        Ok(func)
    }

    /// Parsea `BY <col>[, <col>...]` de la cláusula `GROUP BY`.
    fn parse_group_by(&mut self) -> Result<Vec<String>, RuscaError> {
        self.expect_keyword(Keyword::By)?;
        let mut columns = vec![self.expect_ident()?];
        while self.peek() == Some(&Token::Comma) {
            self.advance();
            columns.push(self.expect_ident()?);
        }
        Ok(columns)
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

    /// Parsea una comparación `expr op expr` o un predicado documental `@>`.
    fn parse_comparison(&mut self) -> Result<Expr, RuscaError> {
        let left = self.parse_expr()?;
        if self.peek() == Some(&Token::AtGt) {
            return self.parse_doc_contains(left);
        }
        let op = self.parse_compare_op()?;
        let right = self.parse_expr()?;
        Ok(Expr::Compare {
            left: Box::new(left),
            op,
            right: Box::new(right),
        })
    }

    /// Parsea `columna @> '{json}'` (el lado izquierdo debe ser una columna).
    fn parse_doc_contains(&mut self, target: Expr) -> Result<Expr, RuscaError> {
        let Expr::Column(column) = target else {
            return Err(self.error("el operador '@>' se aplica a una columna documental"));
        };
        self.expect_token(&Token::AtGt, &format!("{EXPECTED_PREFIX}'@>'"))?;
        let json = self.expect_text()?;
        Ok(Expr::DocContains { column, json })
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

    /// Parsea un operando: columna (o extracción `col -> 'ruta'`) o literal.
    fn parse_expr(&mut self) -> Result<Expr, RuscaError> {
        let expr = match self.peek().cloned() {
            Some(Token::Ident(name)) => Expr::Column(name),
            Some(Token::Int(value)) => Expr::Int(value),
            Some(Token::Float(value)) => Expr::Float(value),
            Some(Token::Text(value)) => Expr::Text(value),
            _ => return Err(self.error(format!("{EXPECTED_PREFIX}una columna o un literal"))),
        };
        self.advance();
        if let Expr::Column(column) = &expr {
            if self.peek() == Some(&Token::Arrow) {
                self.advance();
                let raw = self.expect_text()?;
                let path = doc_path_segments(&raw).map_err(|message| self.error(message))?;
                return Ok(Expr::DocExtract {
                    column: column.clone(),
                    path,
                });
            }
        }
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

    /// Parsea la cláusula `ORDER BY <col> [ASC|DESC]` (`ASC` por defecto).
    fn parse_order_by(&mut self) -> Result<OrderBy, RuscaError> {
        self.expect_keyword(Keyword::By)?;
        let column = self.expect_ident()?;
        let desc = self.match_keyword(Keyword::Desc);
        if !desc {
            self.match_keyword(Keyword::Asc);
        }
        Ok(OrderBy { column, desc })
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
