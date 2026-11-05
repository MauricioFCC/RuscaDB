//! Parser recursive-descent de RQL sobre los tokens del lexer.

use crate::ast::{
    AggFunc, Aggregate, ColumnRef, CompareOp, ConflictAction, ConflictValue, Cte, Delete, Explain,
    Expr, FrameBound, FrameKind, FrameSpec, GraphPath, HavingCondition, Insert, JoinClause,
    JoinKind, KnnClause, KnnMode, OnConflict, OrderBy, PathDirection, PathHop, ProjectedExpr,
    Projection, RecordRef, RelateConflict, Select, SetOpKind, Statement, TraverseClause, Update,
    WindowFunc, WindowItem, WindowSpec,
};
use crate::lexer::{Keyword, Spanned, Token, tokenize};
use crate::limits::check_depth;
use ruscadb_core::RuscaError;

/// Resultado de `FROM`: alias, tabla derivada y `JOIN` opcional.
type FromClause = (String, Option<Box<Select>>, Option<JoinClause>);
/// Resultado de la proyección: columnas, agregados, ventanas, expresiones y paths.
type ProjectionParts = (
    Projection,
    Vec<Aggregate>,
    Vec<WindowItem>,
    Vec<ProjectedExpr>,
    Vec<GraphPath>,
);

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
        Statement::Insert(_)
        | Statement::Update(_)
        | Statement::Delete(_)
        | Statement::Analyze { .. }
        | Statement::DropIndex { .. }
        | Statement::DropTable { .. }
        | Statement::SetOp { .. }
        | Statement::With { .. }
        | Statement::Relate { .. }
        | Statement::Let { .. }
        | Statement::Return(_) => Err(crate::lexer::parse_error(
            "la sentencia no es un SELECT simple; usa parse_statement",
            0,
        )),
    }
}

/// Analiza un script RQL de una o más sentencias separadas por `;`.
///
/// Args:
///     input: Texto del script (`LET ...; SELECT ...; RETURN ...`).
///
/// Returns:
///     Las sentencias en orden, sin el `;` final opcional.
///
/// Errors:
///     [`RuscaError::ParseError`] si una sentencia está mal formada o el
///     script está vacío.
pub fn parse_script(input: &str) -> Result<Vec<Statement>, RuscaError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens, input.len());
    let mut statements = Vec::new();
    while parser.position < parser.tokens.len() {
        statements.push(parser.parse_statement()?);
        if parser.peek() == Some(&Token::Semicolon) {
            parser.advance();
        } else if parser.position < parser.tokens.len() {
            return Err(parser.error("se esperaba ';' entre sentencias del script"));
        }
    }
    if statements.is_empty() {
        return Err(crate::lexer::parse_error(
            "el script está vacío: se esperaba al menos una sentencia",
            0,
        ));
    }
    Ok(statements)
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

/// Parsea `{payload, ts_unix}` de un path de grafo (SPEC-0193).
///
/// Args:
///     raw: Texto crudo del objeto JSON, incluidas las llaves.
///
/// Returns:
///     Los campos pedidos (`payload`, `ts_unix`), sin duplicados.
///
/// Errors:
///     Mensaje accionable si el objeto no es válido o pide un campo no
///     soportado.
fn parse_edge_fields(raw: &str) -> Result<Vec<String>, String> {
    let inner = raw
        .strip_prefix('{')
        .and_then(|text| text.strip_suffix('}'))
        .ok_or_else(|| "campos de arista mal formados: usa {payload, ts_unix}".to_string())?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut fields = Vec::new();
    for part in inner.split(',') {
        let name = part.trim();
        if !matches!(name, "payload" | "ts_unix") {
            return Err(format!(
                "campo de arista '{name}' no soportado; usa payload o ts_unix"
            ));
        }
        if fields.iter().any(|field| field == name) {
            return Err(format!("campo de arista duplicado: '{name}'"));
        }
        fields.push(name.to_string());
    }
    Ok(fields)
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

    /// Token en la posicion `offset` desde el cursor (lookahead sin consumir).
    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens
            .get(self.position + offset)
            .map(|spanned| &spanned.token)
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

    /// Consume un identificador si coincide con `word` (sin distinguir caja).
    ///
    /// Se usa para modificadores opcionales (`USING <modo>`) que no reservan
    /// palabras clave y por tanto no rompen columnas existentes.
    ///
    /// Args:
    ///     word: Texto esperado.
    ///
    /// Returns:
    ///     `true` si lo consumió.
    fn match_word(&mut self, word: &str) -> bool {
        match self.peek() {
            Some(Token::Ident(ident)) if ident.eq_ignore_ascii_case(word) => {
                self.advance();
                true
            }
            _ => false,
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
            Some(Token::Keyword(Keyword::Analyze)) => {
                self.advance();
                let table = self.expect_ident()?;
                Ok(Statement::Analyze { table })
            }
            Some(Token::Keyword(Keyword::Drop)) => {
                self.advance();
                match self.peek() {
                    Some(Token::Keyword(Keyword::Index)) => {
                        self.advance();
                        let table = self.expect_ident()?;
                        self.expect_token(
                            &Token::Dot,
                            &format!("{EXPECTED_PREFIX}'.' en DROP INDEX tabla.columna"),
                        )?;
                        let column = self.expect_ident()?;
                        Ok(Statement::DropIndex { table, column })
                    }
                    Some(Token::Keyword(Keyword::Table)) => {
                        self.advance();
                        let table = self.expect_ident()?;
                        Ok(Statement::DropTable { table })
                    }
                    _ => Err(self.error(format!("{EXPECTED_PREFIX}'INDEX' o 'TABLE' tras DROP"))),
                }
            }
            Some(Token::Keyword(Keyword::Insert)) => Ok(Statement::Insert(self.parse_insert()?)),
            Some(Token::Keyword(Keyword::Update)) => Ok(Statement::Update(self.parse_update()?)),
            Some(Token::Keyword(Keyword::Delete)) => Ok(Statement::Delete(self.parse_delete()?)),
            Some(Token::Keyword(Keyword::With)) => self.parse_with(),
            Some(Token::Keyword(Keyword::Relate)) => self.parse_relate(),
            Some(Token::Keyword(Keyword::Let)) => self.parse_let(),
            Some(Token::Keyword(Keyword::Return)) => {
                self.advance();
                Ok(Statement::Return(self.parse_expr()?))
            }
            _ => self.parse_set_statement(),
        }
    }

    /// Parsea `WITH [RECURSIVE] nombre [(cols)] AS (sentencia) [, ...] cuerpo`.
    ///
    /// Returns:
    ///     [`Statement::With`] con los CTEs y la sentencia que los consume.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si falta un nombre, `AS`, los paréntesis
    ///     o el cuerpo.
    fn parse_with(&mut self) -> Result<Statement, RuscaError> {
        self.expect_keyword(Keyword::With)?;
        let recursive = self.match_keyword(Keyword::Recursive);
        let mut ctes = Vec::new();
        loop {
            let name = self.expect_ident()?;
            let columns = if self.peek() == Some(&Token::LParen) {
                self.advance();
                let columns = self.parse_ident_list()?;
                self.expect_token(
                    &Token::RParen,
                    &format!("{EXPECTED_PREFIX}')' para cerrar las columnas del CTE"),
                )?;
                columns
            } else {
                Vec::new()
            };
            self.expect_keyword(Keyword::As)?;
            self.expect_token(
                &Token::LParen,
                &format!("{EXPECTED_PREFIX}'(' tras AS en el CTE"),
            )?;
            let query = Box::new(self.parse_statement()?);
            self.expect_token(
                &Token::RParen,
                &format!("{EXPECTED_PREFIX}')' para cerrar el CTE"),
            )?;
            ctes.push(Cte {
                name,
                columns,
                query,
                recursive,
            });
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let body = Box::new(self.parse_statement()?);
        Ok(Statement::With { ctes, body })
    }

    /// Parsea `RELATE tabla:id->etiqueta->tabla:id` (SPEC-0185).
    fn parse_relate(&mut self) -> Result<Statement, RuscaError> {
        self.expect_keyword(Keyword::Relate)?;
        let from = self.parse_record_ref()?;
        self.expect_token(
            &Token::Arrow,
            &format!("{EXPECTED_PREFIX}'->' tras el registro origen de RELATE"),
        )?;
        let edge = self.expect_ident()?;
        self.expect_token(
            &Token::Arrow,
            &format!("{EXPECTED_PREFIX}'->' tras la etiqueta de RELATE"),
        )?;
        let to = self.parse_record_ref()?;
        let payload = if self.match_keyword(Keyword::Set) {
            Some(self.expect_json()?)
        } else {
            None
        };
        let ts_unix = if self.match_word("at") {
            Some(self.parse_unsigned::<i64>("se esperaba un entero Unix (segundos) tras AT")?)
        } else {
            None
        };
        let conflict = if self.match_keyword(Keyword::On) {
            if !self.match_word("conflict") {
                return Err(self.error(format!("{EXPECTED_PREFIX}CONFLICT tras ON")));
            }
            if !self.match_word("do") {
                return Err(self.error(format!("{EXPECTED_PREFIX}DO tras ON CONFLICT")));
            }
            if self.match_word("nothing") {
                RelateConflict::DoNothing
            } else if self.match_keyword(Keyword::Update) {
                self.expect_keyword(Keyword::Set)?;
                if !self.match_word("payload") {
                    return Err(self.error(
                        "ON CONFLICT DO UPDATE solo admite 'payload = { ... }'".to_string(),
                    ));
                }
                self.expect_token(
                    &Token::Eq,
                    &format!("{EXPECTED_PREFIX}'=' tras payload en ON CONFLICT"),
                )?;
                RelateConflict::DoUpdate {
                    payload: Some(self.expect_json()?),
                }
            } else {
                return Err(self.error(format!(
                    "{EXPECTED_PREFIX}NOTHING o UPDATE tras ON CONFLICT DO"
                )));
            }
        } else {
            RelateConflict::Default
        };
        Ok(Statement::Relate {
            from,
            edge,
            to,
            payload,
            ts_unix,
            conflict,
        })
    }

    /// Consume un objeto JSON crudo `{ ... }`.
    fn expect_json(&mut self) -> Result<String, RuscaError> {
        match self.peek().cloned() {
            Some(Token::Json(raw)) => {
                self.advance();
                Ok(raw)
            }
            _ => Err(self.error(format!("{EXPECTED_PREFIX}un objeto JSON '{{ ... }}'"))),
        }
    }

    /// Parsea `tabla:id` de una sentencia `RELATE`.
    fn parse_record_ref(&mut self) -> Result<RecordRef, RuscaError> {
        let table = self.expect_ident()?;
        match self.peek().cloned() {
            Some(Token::RecordId(id)) => {
                self.advance();
                Ok(RecordRef { table, id })
            }
            _ => Err(self.error(format!(
                "{EXPECTED_PREFIX}la referencia '{table}:id' (identificador tras ':')"
            ))),
        }
    }

    /// Parsea `LET $nombre = expr` (SPEC-0186).
    fn parse_let(&mut self) -> Result<Statement, RuscaError> {
        self.expect_keyword(Keyword::Let)?;
        let name = match self.peek().cloned() {
            Some(Token::Variable(name)) => {
                self.advance();
                name
            }
            _ => return Err(self.error(format!("{EXPECTED_PREFIX}'$nombre' tras LET"))),
        };
        self.expect_token(
            &Token::Eq,
            &format!("{EXPECTED_PREFIX}'=' tras la variable de LET"),
        )?;
        let expr = self.parse_expr()?;
        Ok(Statement::Let { name, expr })
    }

    /// Parsea `SELECT ... [UNION|INTERSECT|EXCEPT [ALL] SELECT ...]*`
    /// (asociativo por la izquierda; `INTERSECT` no tiene precedencia extra).
    fn parse_set_statement(&mut self) -> Result<Statement, RuscaError> {
        let mut left = Statement::Select(self.parse_select()?);
        loop {
            let op = match self.peek() {
                Some(Token::Keyword(Keyword::Union)) => SetOpKind::Union,
                Some(Token::Keyword(Keyword::Intersect)) => SetOpKind::Intersect,
                Some(Token::Keyword(Keyword::Except)) => SetOpKind::Except,
                _ => break,
            };
            self.advance();
            let all = self.match_keyword(Keyword::All);
            let right = Statement::Select(self.parse_select()?);
            left = Statement::SetOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
                all,
            };
        }
        Ok(left)
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
        let on_conflict = self.parse_on_conflict()?;
        let returning = self.parse_returning()?;
        Ok(Insert {
            table,
            columns,
            rows,
            returning,
            on_conflict,
        })
    }

    /// Parsea la cláusula opcional `ON CONFLICT (cols) DO NOTHING | DO UPDATE
    /// SET col = literal | excluded.col` (SPEC-0107).
    ///
    /// Returns:
    ///     `Some(OnConflict)` si la cláusula está presente, `None` si no.
    fn parse_on_conflict(&mut self) -> Result<Option<OnConflict>, RuscaError> {
        if !self.match_keyword(Keyword::On) {
            return Ok(None);
        }
        // `CONFLICT` es identificador contextual (sin keyword nuevo).
        let conflict = self.expect_ident()?;
        if !conflict.eq_ignore_ascii_case("conflict") {
            return Err(self.error(format!("{EXPECTED_PREFIX}'CONFLICT' tras ON en el upsert")));
        }
        self.expect_token(
            &Token::LParen,
            &format!("{EXPECTED_PREFIX}'(' tras CONFLICT"),
        )?;
        let target = self.parse_ident_list()?;
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar la lista de conflicto"),
        )?;
        // `DO` es identificador contextual.
        let do_kw = self.expect_ident()?;
        if !do_kw.eq_ignore_ascii_case("do") {
            return Err(self.error(format!("{EXPECTED_PREFIX}'DO' tras el target de conflicto")));
        }
        // NOTHING (identificador contextual) o UPDATE (keyword).
        if let Some(Token::Ident(name)) = self.peek()
            && name.eq_ignore_ascii_case("nothing")
        {
            self.advance();
            return Ok(Some(OnConflict {
                target,
                action: ConflictAction::DoNothing,
            }));
        }
        self.expect_keyword(Keyword::Update)?;
        self.expect_keyword(Keyword::Set)?;
        let mut assignments = Vec::new();
        loop {
            let column = self.expect_ident()?;
            self.expect_token(
                &Token::Eq,
                &format!("{EXPECTED_PREFIX}'=' en SET del upsert"),
            )?;
            let value = self.parse_conflict_value()?;
            assignments.push((column, value));
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(Some(OnConflict {
            target,
            action: ConflictAction::DoUpdate(assignments),
        }))
    }

    /// Parsea el lado derecho de un `SET` del upsert: literal o `excluded.col`.
    fn parse_conflict_value(&mut self) -> Result<ConflictValue, RuscaError> {
        if let Some(Token::Ident(name)) = self.peek()
            && name.eq_ignore_ascii_case("excluded")
        {
            self.advance();
            self.expect_token(
                &Token::Dot,
                &format!("{EXPECTED_PREFIX}'.' en excluded.col"),
            )?;
            let column = self.expect_ident()?;
            return Ok(ConflictValue::Excluded(column));
        }
        let literal = self.parse_literal()?;
        Ok(ConflictValue::Literal(literal))
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
        let returning = self.parse_returning()?;
        Ok(Update {
            table,
            assignments,
            filter,
            returning,
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
        let returning = self.parse_returning()?;
        Ok(Delete {
            table,
            filter,
            returning,
        })
    }

    /// Parsea la cláusula opcional `RETURNING *|<cols>` (SPEC-0102).
    ///
    /// Returns:
    ///     `Some(["*"])` para `RETURNING *`, `Some(cols)` para una lista de
    ///     columnas, o `None` si no hay cláusula.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si `RETURNING` no va seguido de `*` o de
    ///     una lista de identificadores.
    fn parse_returning(&mut self) -> Result<Option<Vec<String>>, RuscaError> {
        if !self.match_keyword(Keyword::Returning) {
            return Ok(None);
        }
        if self.peek() == Some(&Token::Star) {
            self.advance();
            return Ok(Some(vec!["*".to_string()]));
        }
        let columns = self.parse_ident_list()?;
        Ok(Some(columns))
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
        let distinct = self.match_keyword(Keyword::Distinct);
        let (projection, aggregates, windows, expressions, paths) = self.parse_projection()?;
        self.expect_keyword(Keyword::From)?;
        let (from, from_derived, join) = self.parse_from()?;
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
        // `HAVING` exige `GROUP BY` (SPEC-0051, AC-0051-03).
        let having = if self.match_keyword(Keyword::Having) {
            if group_by.is_empty() {
                return Err(self.error("HAVING exige GROUP BY".to_string()));
            }
            self.parse_having()?
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
        // `OFFSET` (SPEC-0096): paginación; puede aparecer sin `LIMIT`
        // (semántica Postgres: `OFFSET n` sola descarta las primeras n filas).
        let offset = if self.match_keyword(Keyword::Offset) {
            Some(self.parse_limit()?)
        } else {
            None
        };
        let select = Select {
            distinct,
            projection,
            aggregates,
            windows,
            expressions,
            paths,
            from,
            from_derived,
            join,
            filter,
            knn,
            traverse,
            group_by,
            having,
            order_by,
            limit,
            offset,
        };
        // Guard clause anti-DoS: el árbol AND anidado crece con la cadena, así
        // que la profundidad se valida aquí, antes de planificar (SPEC-0054).
        check_depth(&select).map_err(|message| self.error(message))?;
        Ok(select)
    }

    /// Parsea la cláusula `FROM`: tabla o tabla derivada + `JOIN` opcional.
    ///
    /// Returns:
    ///     `(alias, derived, join)`: el nombre de la tabla (o alias de la
    ///     subconsulta), la tabla derivada `FROM (subquery) AS alias`
    ///     (SPEC-0162) y la cláusula `JOIN`, si la hay.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si el `JOIN` está mal formado o si se
    ///     intenta unir una tabla derivada (fuera de alcance de SPEC-0162).
    fn parse_from(&mut self) -> Result<FromClause, RuscaError> {
        let (from, derived) = if self.peek() == Some(&Token::LParen)
            && matches!(self.peek_at(1), Some(Token::Keyword(Keyword::Select)))
        {
            self.advance();
            let query = self.parse_select()?;
            self.expect_token(
                &Token::RParen,
                &format!("{EXPECTED_PREFIX}')' para cerrar la tabla derivada"),
            )?;
            self.expect_keyword(Keyword::As)?;
            let alias = self.expect_ident()?;
            (alias, Some(Box::new(query)))
        } else {
            (self.expect_ident()?, None)
        };
        // Tipo de JOIN (SPEC-0099/SPEC-0103).
        let kind = if self.match_keyword(Keyword::Left) {
            self.match_keyword(Keyword::Outer);
            JoinKind::Left
        } else if self.match_keyword(Keyword::Right) {
            self.match_keyword(Keyword::Outer);
            JoinKind::Right
        } else if self.match_keyword(Keyword::Full) {
            self.match_keyword(Keyword::Outer);
            JoinKind::Full
        } else if self.match_keyword(Keyword::Cross) {
            JoinKind::Cross
        } else {
            self.match_keyword(Keyword::Inner);
            JoinKind::Inner
        };
        let join = if self.match_keyword(Keyword::Join) {
            if derived.is_some() {
                return Err(self.error(
                    "JOIN sobre una tabla derivada queda fuera de alcance de SPEC-0162: materialízala con WITH o una subconsulta independiente"
                        .to_string(),
                ));
            }
            Some(self.parse_join(&from, kind)?)
        } else if kind != JoinKind::Inner {
            return Err(self.error(
                "LEFT/RIGHT/FULL/CROSS/INNER exige un JOIN a continuación ('<tipo> JOIN <tabla> [ON a.x = b.y]')"
                    .to_string(),
            ));
        } else {
            None
        };
        Ok((from, derived, join))
    }

    /// Parsea `JOIN <tabla> ON <a.x> = <b.y>` (solo INNER, una igualdad).
    ///
    /// Args:
    ///     from: Tabla exterior ya parseada (para rechazar el self-join).
    ///
    /// Returns:
    ///     La cláusula [`JoinClause`] con ambos lados cualificados.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si el `ON` no es una única igualdad
    ///     `tabla.columna = tabla.columna`, si hay multi-condición (`AND`),
    ///     o si es un self-join sin alias (ambiguo, fuera de alcance).
    fn parse_join(&mut self, from: &str, kind: JoinKind) -> Result<JoinClause, RuscaError> {
        let table = self.expect_ident()?;
        if table == from {
            return Err(self.error(
                "self-join sin alias no soportado: usa alias distintos para cada lado (fuera de alcance de SPEC-0052)",
            ));
        }
        if kind == JoinKind::Cross {
            return Ok(JoinClause {
                kind,
                table,
                left: None,
                right: None,
            });
        }
        self.expect_keyword(Keyword::On)?;
        let left = self.parse_join_side()?;
        if self.peek() != Some(&Token::Eq) {
            return Err(self.error(
                "el ON del JOIN solo admite una igualdad 'a.x = b.y' (non-equi fuera de alcance de SPEC-0052)",
            ));
        }
        self.advance();
        let right = self.parse_join_side()?;
        if self.peek() == Some(&Token::Keyword(Keyword::And)) {
            return Err(self.error(
                "el ON del JOIN solo admite una condición (multi-condición con AND fuera de alcance de SPEC-0052)",
            ));
        }
        Ok(JoinClause {
            kind,
            table,
            left: Some(left),
            right: Some(right),
        })
    }

    /// Parsea un lado del `ON`: referencia cualificada `tabla.columna`.
    ///
    /// Returns:
    ///     La referencia [`ColumnRef`] con tabla y columna.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si falta la tabla, el `.` o la columna.
    fn parse_join_side(&mut self) -> Result<ColumnRef, RuscaError> {
        if !matches!(self.peek(), Some(Token::Ident(_))) {
            return Err(self.error(
                "el ON del JOIN exige columnas cualificadas 'tabla.columna' (p. ej. ON a.id = b.a_id)",
            ));
        }
        let table = self.expect_ident()?;
        if self.peek() != Some(&Token::Dot) {
            return Err(self.error(
                "el ON del JOIN exige columnas cualificadas 'tabla.columna' (p. ej. ON a.id = b.a_id)",
            ));
        }
        self.advance();
        let column = self.expect_ident()?;
        Ok(ColumnRef { table, column })
    }

    /// Parsea la proyección: `*` o una lista de columnas, agregados, ventanas,
    /// expresiones y paths de grafo.
    ///
    /// Returns:
    ///     `(proyección, agregados, ventanas, expresiones, paths)`; cada grupo
    ///     conserva su orden relativo y el executor los concatena en el orden
    ///     columnas → ventanas → expresiones → paths → agregados.
    fn parse_projection(&mut self) -> Result<ProjectionParts, RuscaError> {
        if self.peek() == Some(&Token::Star) {
            self.advance();
            return Ok((
                Projection::All,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ));
        }
        let mut columns = Vec::new();
        let mut aggregates = Vec::new();
        let mut windows = Vec::new();
        let mut expressions = Vec::new();
        let mut paths = Vec::new();
        loop {
            if matches!(self.peek(), Some(Token::Arrow | Token::LtArrow)) {
                paths.push(self.parse_graph_path()?);
            } else if self.window_ahead() {
                windows.push(self.parse_window_item()?);
            } else if self.is_aggregate_function() {
                aggregates.push(self.parse_aggregate()?);
            } else if matches!(
                self.peek(),
                Some(Token::Keyword(
                    Keyword::If | Keyword::Case | Keyword::Coalesce | Keyword::Nullif
                ))
            ) {
                let expr = self.parse_expr()?;
                let alias = if self.match_keyword(Keyword::As) {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                expressions.push(ProjectedExpr { expr, alias });
            } else {
                columns.push(self.parse_column_name()?);
            }
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok((
            Projection::Columns(columns),
            aggregates,
            windows,
            expressions,
            paths,
        ))
    }

    /// Indica si la función actual va seguida de `OVER (...)`, sin consumir.
    ///
    /// Busca el paréntesis de cierre de la llamada y comprueba que el token
    /// siguiente es el identificador contextual `OVER` (SPEC-0165).
    fn window_ahead(&self) -> bool {
        let is_window_name = match self.peek() {
            Some(Token::Keyword(
                Keyword::Count | Keyword::Sum | Keyword::Avg | Keyword::Min | Keyword::Max,
            )) => true,
            Some(Token::Ident(name)) => matches!(
                name.to_ascii_lowercase().as_str(),
                "row_number"
                    | "rank"
                    | "dense_rank"
                    | "ntile"
                    | "lag"
                    | "lead"
                    | "first_value"
                    | "last_value"
                    | "nth_value"
            ),
            _ => false,
        };
        if !is_window_name || self.peek_at(1) != Some(&Token::LParen) {
            return false;
        }
        let mut depth = 0usize;
        for offset in 1..self.tokens.len().saturating_sub(self.position) {
            match self.peek_at(offset) {
                Some(Token::LParen) => depth += 1,
                Some(Token::RParen) => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(
                            self.peek_at(offset + 1),
                            Some(Token::Ident(name)) if name.eq_ignore_ascii_case("over")
                        );
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Parsea una función de ventana completa `func(args) OVER (...)`.
    fn parse_window_item(&mut self) -> Result<WindowItem, RuscaError> {
        let func = self.parse_window_func()?;
        self.expect_token(
            &Token::LParen,
            &format!("{EXPECTED_PREFIX}'(' tras {}", func.as_str()),
        )?;
        let (arg, offset) = self.parse_window_args(func)?;
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar {}", func.as_str()),
        )?;
        if !self.match_word("over") {
            return Err(self.error(format!("{EXPECTED_PREFIX}OVER tras {}", func.as_str())));
        }
        self.expect_token(&Token::LParen, &format!("{EXPECTED_PREFIX}'(' tras OVER"))?;
        let over = self.parse_window_spec()?;
        let alias = if self.match_keyword(Keyword::As) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(WindowItem {
            func,
            arg,
            offset,
            over,
            alias,
        })
    }

    /// Resuelve el nombre de una función de ventana y avanza el cursor.
    fn parse_window_func(&mut self) -> Result<WindowFunc, RuscaError> {
        let func = match self.peek() {
            Some(Token::Keyword(Keyword::Count)) => WindowFunc::Count,
            Some(Token::Keyword(Keyword::Sum)) => WindowFunc::Sum,
            Some(Token::Keyword(Keyword::Avg)) => WindowFunc::Avg,
            Some(Token::Keyword(Keyword::Min)) => WindowFunc::Min,
            Some(Token::Keyword(Keyword::Max)) => WindowFunc::Max,
            Some(Token::Ident(name)) => match name.to_ascii_lowercase().as_str() {
                "row_number" => WindowFunc::RowNumber,
                "rank" => WindowFunc::Rank,
                "dense_rank" => WindowFunc::DenseRank,
                "ntile" => WindowFunc::Ntile,
                "lag" => WindowFunc::Lag,
                "lead" => WindowFunc::Lead,
                "first_value" => WindowFunc::FirstValue,
                "last_value" => WindowFunc::LastValue,
                "nth_value" => WindowFunc::NthValue,
                _ => {
                    return Err(
                        self.error(format!("{EXPECTED_PREFIX}una función de ventana válida"))
                    );
                }
            },
            _ => return Err(self.error(format!("{EXPECTED_PREFIX}una función de ventana"))),
        };
        self.advance();
        Ok(func)
    }

    /// Parsea los argumentos `(arg[, n])` de una función de ventana.
    fn parse_window_args(
        &mut self,
        func: WindowFunc,
    ) -> Result<(Option<String>, Option<u64>), RuscaError> {
        match func {
            WindowFunc::RowNumber | WindowFunc::Rank | WindowFunc::DenseRank => Ok((None, None)),
            WindowFunc::Ntile => {
                let buckets =
                    self.parse_unsigned::<u64>("se esperaba un entero positivo en NTILE(n)")?;
                Ok((None, Some(buckets)))
            }
            WindowFunc::Lag | WindowFunc::Lead => {
                let arg = self.expect_ident()?;
                let offset = if self.peek() == Some(&Token::Comma) {
                    self.advance();
                    Some(self.parse_unsigned::<u64>(
                        "se esperaba un entero no negativo como offset de LAG/LEAD",
                    )?)
                } else {
                    None
                };
                Ok((Some(arg), offset))
            }
            WindowFunc::NthValue => {
                let arg = self.expect_ident()?;
                self.expect_token(
                    &Token::Comma,
                    &format!("{EXPECTED_PREFIX}',' entre columna y n en NTH_VALUE"),
                )?;
                let position = self
                    .parse_unsigned::<u64>("se esperaba un entero positivo como n de NTH_VALUE")?;
                Ok((Some(arg), Some(position)))
            }
            WindowFunc::FirstValue
            | WindowFunc::LastValue
            | WindowFunc::Sum
            | WindowFunc::Avg
            | WindowFunc::Count
            | WindowFunc::Min
            | WindowFunc::Max => {
                let arg = self.expect_ident()?;
                Ok((Some(arg), None))
            }
        }
    }

    /// Parsea la especificación interior de `OVER (...)`.
    fn parse_window_spec(&mut self) -> Result<WindowSpec, RuscaError> {
        let mut spec = WindowSpec::default();
        if self.match_word("partition") {
            self.expect_keyword(Keyword::By)?;
            spec.partition_by = self.parse_ident_list()?;
        }
        if self.match_keyword(Keyword::Order) {
            spec.order_by = Some(self.parse_order_by()?);
        }
        if self.match_word("rows") {
            spec.frame = Some(self.parse_frame(FrameKind::Rows)?);
        } else if self.match_word("range") {
            spec.frame = Some(self.parse_frame(FrameKind::Range)?);
        }
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar OVER"),
        )?;
        Ok(spec)
    }

    /// Parsea el marco `[BETWEEN] <bound> [AND <bound>]`.
    fn parse_frame(&mut self, kind: FrameKind) -> Result<FrameSpec, RuscaError> {
        let between = self.match_word("between");
        let start = self.parse_frame_bound()?;
        let end = if between {
            if !self.match_keyword(Keyword::And) {
                return Err(self.error(format!("{EXPECTED_PREFIX}AND en el marco de la ventana")));
            }
            self.parse_frame_bound()?
        } else {
            FrameBound::CurrentRow
        };
        Ok(FrameSpec { kind, start, end })
    }

    /// Parsea un límite de marco (`UNBOUNDED PRECEDING`, `n FOLLOWING`, ...).
    fn parse_frame_bound(&mut self) -> Result<FrameBound, RuscaError> {
        if self.match_word("unbounded") {
            if self.match_word("preceding") {
                return Ok(FrameBound::UnboundedPreceding);
            }
            if self.match_word("following") {
                return Ok(FrameBound::UnboundedFollowing);
            }
            return Err(self.error(format!(
                "{EXPECTED_PREFIX}PRECEDING o FOLLOWING tras UNBOUNDED"
            )));
        }
        if self.match_word("current") {
            if self.match_word("row") {
                return Ok(FrameBound::CurrentRow);
            }
            return Err(self.error(format!("{EXPECTED_PREFIX}ROW tras CURRENT")));
        }
        let value =
            self.parse_unsigned::<u64>("se esperaba un entero en el marco de la ventana")?;
        if self.match_word("preceding") {
            return Ok(FrameBound::Preceding(value));
        }
        if self.match_word("following") {
            return Ok(FrameBound::Following(value));
        }
        Err(self.error(format!(
            "{EXPECTED_PREFIX}PRECEDING o FOLLOWING tras {value}"
        )))
    }

    /// Parsea un path de grafo de uno o más saltos:
    /// `->label->tabla[->label->tabla...]` o `<-label<-tabla[...]`.
    fn parse_graph_path(&mut self) -> Result<GraphPath, RuscaError> {
        let mut hops = vec![self.parse_path_hop()?];
        while matches!(self.peek(), Some(Token::Arrow | Token::LtArrow)) {
            hops.push(self.parse_path_hop()?);
        }
        let alias = if self.match_keyword(Keyword::As) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(GraphPath { hops, alias })
    }

    /// Parsea un salto `->label->tabla` o `<-label<-tabla`.
    fn parse_path_hop(&mut self) -> Result<PathHop, RuscaError> {
        let direction = if self.peek() == Some(&Token::Arrow) {
            PathDirection::Out
        } else {
            PathDirection::In
        };
        let arrow = match direction {
            PathDirection::Out => Token::Arrow,
            PathDirection::In => Token::LtArrow,
        };
        self.expect_token(&arrow, &format!("{EXPECTED_PREFIX}'->' o '<-' en el path"))?;
        let label = self.expect_ident()?;
        let edge_fields = match self.peek().cloned() {
            Some(Token::Json(raw)) => {
                self.advance();
                parse_edge_fields(&raw).map_err(|message| self.error(message))?
            }
            _ => Vec::new(),
        };
        self.expect_token(
            &arrow,
            &format!("{EXPECTED_PREFIX}la flecha de cierre del path '{label}'"),
        )?;
        let table = self.expect_ident()?;
        Ok(PathHop {
            direction,
            label,
            table,
            edge_fields,
        })
    }

    /// Parsea un nombre de columna proyectada: `col` o `tabla.col` (SPEC-0052).
    ///
    /// Returns:
    ///     El nombre tal cual (`col`) o cualificado (`tabla.col`).
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si no hay un identificador (o falta la
    ///     columna tras el `.`).
    fn parse_column_name(&mut self) -> Result<String, RuscaError> {
        let first = self.expect_ident()?;
        if self.peek() != Some(&Token::Dot) {
            return Ok(first);
        }
        self.advance();
        let second = self.expect_ident()?;
        Ok(format!("{first}.{second}"))
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

    /// Parsea un agregado `FUNC(<*|columna|CASE ...>) [AS alias]`.
    fn parse_aggregate(&mut self) -> Result<Aggregate, RuscaError> {
        let (func, column, expr) = self.parse_agg_call()?;
        let alias = if self.match_keyword(Keyword::As) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(Aggregate {
            func,
            column,
            alias,
            expr,
        })
    }

    /// Parsea la llamada `FUNC(<*|columna|CASE ...>)` sin alias.
    ///
    /// Returns:
    ///     `(función, columna, expresión)`: `COUNT(*)` se normaliza a
    ///     `(CountStar, None, None)`; `CASE` produce `(func, None, Some(expr))`.
    ///     Solo `COUNT` admite `'*'`.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si falta el paréntesis o si una función
    ///     distinta de `COUNT` usa `'*'`.
    fn parse_agg_call(&mut self) -> Result<(AggFunc, Option<String>, Option<Expr>), RuscaError> {
        let func = self.parse_agg_func()?;
        self.expect_token(
            &Token::LParen,
            &format!("{EXPECTED_PREFIX}'(' tras {}", func.as_str()),
        )?;
        let (column, expr) = if self.peek() == Some(&Token::Star) {
            self.advance();
            (None, None)
        } else if matches!(
            self.peek(),
            Some(Token::Keyword(
                Keyword::Case | Keyword::Coalesce | Keyword::Nullif
            ))
        ) {
            (None, Some(self.parse_expr()?))
        } else {
            (Some(self.expect_ident()?), None)
        };
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar {}", func.as_str()),
        )?;
        let func = match (func, column.is_some() || expr.is_some()) {
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
        Ok((func, column, expr))
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

    /// Parsea `CASE [operando] WHEN ... THEN ... [ELSE ...] END`.
    ///
    /// Sin operando, cada `WHEN` es un predicado (admite `AND`); con operando,
    /// cada `WHEN` es un valor que se compara por igualdad.
    ///
    /// Returns:
    ///     [`Expr::Case`] con las ramas en orden.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si falta `WHEN`/`THEN`/`END` o no hay ramas.
    fn parse_case(&mut self) -> Result<Expr, RuscaError> {
        self.expect_keyword(Keyword::Case)?;
        // `CASE END` (sin WHEN) tambien entra aqui para dar el error de rama
        // faltante en vez de "se esperaba una columna".
        let operand = if matches!(
            self.peek(),
            Some(Token::Keyword(Keyword::When | Keyword::End))
        ) {
            None
        } else {
            Some(Box::new(self.parse_expr()?))
        };
        let mut branches = Vec::new();
        while self.match_keyword(Keyword::When) {
            let condition = if operand.is_some() {
                self.parse_expr()?
            } else {
                self.parse_filter()?
            };
            self.expect_keyword(Keyword::Then)?;
            let result = self.parse_expr()?;
            branches.push((condition, result));
        }
        if branches.is_empty() {
            return Err(self.error(format!("{EXPECTED_PREFIX}al menos un WHEN en CASE")));
        }
        let otherwise = if self.match_keyword(Keyword::Else) {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        self.expect_keyword(Keyword::End)?;
        Ok(Expr::Case {
            operand,
            branches,
            otherwise,
        })
    }

    /// Parsea `IF <condición> THEN <expr> [ELSE <expr>] END` como `CASE`
    /// (SPEC-0186).
    ///
    /// Returns:
    ///     [`Expr::Case`] con una rama y el `ELSE` opcional.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si falta `THEN` o `END`.
    fn parse_if(&mut self) -> Result<Expr, RuscaError> {
        self.expect_keyword(Keyword::If)?;
        let condition = self.parse_filter()?;
        self.expect_keyword(Keyword::Then)?;
        let then = self.parse_expr()?;
        let otherwise = if self.match_keyword(Keyword::Else) {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        self.expect_keyword(Keyword::End)?;
        Ok(Expr::Case {
            operand: None,
            branches: vec![(condition, then)],
            otherwise,
        })
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

    /// Parsea `HAVING <agregado> <op> <literal> [AND ...]` (SPEC-0051).
    ///
    /// Returns:
    ///     Las condiciones del `HAVING` en su orden de aparición.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si un agregado está mal formado o si el
    ///     lado derecho no es un literal escalar.
    fn parse_having(&mut self) -> Result<Vec<HavingCondition>, RuscaError> {
        let mut conditions = vec![self.parse_having_condition()?];
        while self.match_keyword(Keyword::And) {
            conditions.push(self.parse_having_condition()?);
        }
        Ok(conditions)
    }

    /// Parsea una condición `<agregado> <op> <literal>` del `HAVING`.
    ///
    /// Returns:
    ///     La condición con el agregado normalizado (`COUNT(*)` → `CountStar`).
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] ante agregado u operador inválidos.
    fn parse_having_condition(&mut self) -> Result<HavingCondition, RuscaError> {
        let (func, column, expr) = self.parse_agg_call()?;
        if expr.is_some() {
            return Err(self.error("HAVING no admite CASE; usa una columna o el alias"));
        }
        let op = self.parse_compare_op()?;
        let literal = self.parse_literal()?;
        Ok(HavingCondition {
            func,
            column,
            op,
            literal,
        })
    }

    /// Parsea el filtro `WHERE` con precedencia SQL: `NOT` > `AND` > `OR`.
    fn parse_filter(&mut self) -> Result<Expr, RuscaError> {
        let mut expr = self.parse_and()?;
        while self.match_keyword(Keyword::Or) {
            let right = self.parse_and()?;
            expr = Expr::Or(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    /// Parsea una conjuncion `AND` (asociativa por la izquierda).
    fn parse_and(&mut self) -> Result<Expr, RuscaError> {
        let mut expr = self.parse_not()?;
        while self.match_keyword(Keyword::And) {
            let right = self.parse_not()?;
            expr = Expr::And(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    /// Parsea `NOT <predicado>` (prefijo, asociativo a la derecha).
    fn parse_not(&mut self) -> Result<Expr, RuscaError> {
        if self.match_keyword(Keyword::Not) {
            return Ok(Expr::Not(Box::new(self.parse_not()?)));
        }
        self.parse_predicate()
    }

    /// Parsea un predicado: `MATCH(...)` o una comparación escalar.
    fn parse_predicate(&mut self) -> Result<Expr, RuscaError> {
        if self.match_keyword(Keyword::Match) {
            return self.parse_match();
        }
        if self.match_keyword(Keyword::Exists) {
            self.expect_token(&Token::LParen, &format!("{EXPECTED_PREFIX}'(' tras EXISTS"))?;
            let query = self.parse_select()?;
            self.expect_token(
                &Token::RParen,
                &format!("{EXPECTED_PREFIX}')' para cerrar EXISTS"),
            )?;
            return Ok(Expr::Exists {
                query: Box::new(query),
                negated: false,
            });
        }
        if matches!(self.peek(), Some(Token::Bool(_))) {
            return self.parse_expr();
        }
        if self.peek() == Some(&Token::LParen) {
            self.advance();
            let inner = self.parse_filter()?;
            self.expect_token(
                &Token::RParen,
                &format!("{EXPECTED_PREFIX}')' para cerrar el predicado"),
            )?;
            return Ok(inner);
        }
        // `WHERE col` sobre una columna booleana: identificador seguido del
        // final del predicado (AND/GROUP/ORDER/LIMIT/OFFSET o fin de consulta).
        if matches!(self.peek(), Some(Token::Ident(_))) && self.is_bare_column_predicate() {
            return self.parse_expr();
        }
        self.parse_comparison()
    }

    /// Indica si el identificador actual es un predicado booleano desnudo.
    fn is_bare_column_predicate(&self) -> bool {
        matches!(
            self.peek_at(1),
            None | Some(Token::Keyword(
                Keyword::And | Keyword::Group | Keyword::Order | Keyword::Limit | Keyword::Offset
            ))
        )
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
        let negated = self.match_keyword(Keyword::Not);
        if self.match_keyword(Keyword::In) {
            self.expect_token(&Token::LParen, &format!("{EXPECTED_PREFIX}'(' tras IN"))?;
            if self.peek() == Some(&Token::Keyword(Keyword::Select)) {
                let query = self.parse_select()?;
                self.expect_token(
                    &Token::RParen,
                    &format!("{EXPECTED_PREFIX}')' para cerrar la subconsulta"),
                )?;
                return Ok(Expr::InSubquery {
                    expr: Box::new(left),
                    query: Box::new(query),
                    negated,
                });
            }
            let mut list = Vec::new();
            if self.peek() != Some(&Token::RParen) {
                loop {
                    list.push(self.parse_expr()?);
                    if self.peek() == Some(&Token::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect_token(
                &Token::RParen,
                &format!("{EXPECTED_PREFIX}')' para cerrar la lista"),
            )?;
            return Ok(Expr::InList {
                expr: Box::new(left),
                list,
                negated,
            });
        }
        if self.match_keyword(Keyword::Between) {
            let low = self.parse_expr()?;
            self.expect_keyword(Keyword::And)?;
            let high = self.parse_expr()?;
            return Ok(Expr::Between {
                expr: Box::new(left),
                low: Box::new(low),
                high: Box::new(high),
                negated,
            });
        }
        let like_kind = if self.match_keyword(Keyword::Like) {
            Some(false)
        } else if self.match_keyword(Keyword::ILike) {
            Some(true)
        } else {
            None
        };
        if let Some(case_insensitive) = like_kind {
            let pattern = self.parse_expr()?;
            return Ok(Expr::Like {
                expr: Box::new(left),
                pattern: Box::new(pattern),
                case_insensitive,
                negated,
            });
        }
        if negated {
            return Err(self.error(format!("{EXPECTED_PREFIX}IN, BETWEEN o LIKE tras NOT")));
        }
        if self.match_word("is") {
            let negated = self.match_keyword(Keyword::Not);
            return match self.peek().cloned() {
                Some(Token::Bool(expected)) => {
                    self.advance();
                    let op = if negated {
                        CompareOp::NotEq
                    } else {
                        CompareOp::Eq
                    };
                    Ok(Expr::Compare {
                        left: Box::new(left),
                        op,
                        right: Box::new(Expr::Bool(expected)),
                    })
                }
                Some(Token::Keyword(Keyword::Null)) => {
                    self.advance();
                    Ok(Expr::IsNull {
                        expr: Box::new(left),
                        negated,
                    })
                }
                _ => Err(self.error(format!("{EXPECTED_PREFIX}TRUE, FALSE o NULL tras IS"))),
            };
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

    /// Parsea un operando: columna (o extracción `col -> 'ruta'`), literal o `CASE`.
    fn parse_expr(&mut self) -> Result<Expr, RuscaError> {
        if self.peek() == Some(&Token::LParen)
            && matches!(self.peek_at(1), Some(Token::Keyword(Keyword::Select)))
        {
            self.advance();
            let query = self.parse_select()?;
            self.expect_token(
                &Token::RParen,
                &format!("{EXPECTED_PREFIX}')' para cerrar la subconsulta"),
            )?;
            return Ok(Expr::ScalarSubquery(Box::new(query)));
        }
        let expr = match self.peek().cloned() {
            Some(Token::Keyword(Keyword::Case)) => return self.parse_case(),
            Some(Token::Keyword(Keyword::If)) => return self.parse_if(),
            Some(Token::Variable(name)) => Expr::Column(format!("${name}")),
            Some(Token::Keyword(Keyword::Coalesce)) => {
                self.advance();
                return self.parse_call_list().map(Expr::Coalesce);
            }
            Some(Token::Keyword(Keyword::Nullif)) => {
                self.advance();
                let mut items = self.parse_call_list()?;
                if items.len() != 2 {
                    return Err(self.error("NULLIF exige exactamente 2 argumentos"));
                }
                let Some(right) = items.pop() else {
                    return Err(self.error("NULLIF exige exactamente 2 argumentos"));
                };
                let Some(left) = items.pop() else {
                    return Err(self.error("NULLIF exige exactamente 2 argumentos"));
                };
                return Ok(Expr::Nullif(Box::new(left), Box::new(right)));
            }
            Some(Token::Ident(name)) => Expr::Column(name),
            Some(Token::Int(value)) => Expr::Int(value),
            Some(Token::Float(value)) => Expr::Float(value),
            Some(Token::Text(value)) => Expr::Text(value),
            Some(Token::Bool(value)) => Expr::Bool(value),
            Some(Token::Keyword(Keyword::Null)) => Expr::Null,
            _ => return Err(self.error(format!("{EXPECTED_PREFIX}una columna o un literal"))),
        };
        self.advance();
        if let Expr::Column(column) = &expr
            && self.peek() == Some(&Token::Arrow)
        {
            self.advance();
            let raw = self.expect_text()?;
            let path = doc_path_segments(&raw).map_err(|message| self.error(message))?;
            return Ok(Expr::DocExtract {
                column: column.clone(),
                path,
            });
        }
        Ok(expr)
    }

    /// Parsea la cláusula `KNN <col> <|k|> [v1, v2, ...] [USING <modo>]`.
    ///
    /// `USING` es opcional y no reserva palabra clave: se acepta como
    /// identificador para no romper columnas llamadas `using`.
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
        let mode = if self.match_word("using") {
            let raw = self.expect_ident()?;
            KnnMode::from_text(&raw).ok_or_else(|| {
                self.error(format!(
                    "{EXPECTED_PREFIX}AUTO, QUANTIZED, FILTERED o EXACT tras USING"
                ))
            })?
        } else {
            KnnMode::Auto
        };
        Ok(KnnClause {
            column,
            k,
            query,
            mode,
        })
    }

    /// Parsea la cláusula `TRAVERSE <col> DEPTH <n>`.
    fn parse_traverse(&mut self) -> Result<TraverseClause, RuscaError> {
        let column = self.expect_ident()?;
        self.expect_keyword(Keyword::Depth)?;
        let depth = self.parse_unsigned::<u16>(DEPTH_MESSAGE)?;
        Ok(TraverseClause { column, depth })
    }

    /// Parsea la cláusula `ORDER BY <col> [ASC|DESC]` (`ASC` por defecto).
    ///
    /// Acepta la forma cualificada `tabla.columna` (necesaria para que el
    /// `ORDER BY` tras un `JOIN` llegue al executor, que lo rechaza con un
    /// error accionable que menciona `JOIN`, SPEC-0052).
    fn parse_order_by(&mut self) -> Result<OrderBy, RuscaError> {
        self.expect_keyword(Keyword::By)?;
        let first = self.expect_ident()?;
        let column = if self.peek() == Some(&Token::Dot) {
            self.advance();
            let second = self.expect_ident()?;
            format!("{first}.{second}")
        } else {
            first
        };
        let desc = self.match_keyword(Keyword::Desc);
        if !desc {
            self.match_keyword(Keyword::Asc);
        }
        Ok(OrderBy { column, desc })
    }

    /// Parsea `(expr, expr, ...)` para `COALESCE`/`NULLIF`/`IN`.
    fn parse_call_list(&mut self) -> Result<Vec<Expr>, RuscaError> {
        self.expect_token(
            &Token::LParen,
            &format!("{EXPECTED_PREFIX}'(' para la lista de expresiones"),
        )?;
        let mut items = Vec::new();
        if self.peek() == Some(&Token::RParen) {
            self.advance();
            return Ok(items);
        }
        loop {
            items.push(self.parse_expr()?);
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect_token(
            &Token::RParen,
            &format!("{EXPECTED_PREFIX}')' para cerrar la lista"),
        )?;
        Ok(items)
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
