//! Materializacion de subconsultas no correlacionadas (SPEC-0162).

use super::*;

/// Sustituye subconsultas por literales ejecutandolas una sola vez.
///
/// Soporta `IN (SELECT ...)`, `(SELECT ...)` escalar y `[NOT] EXISTS`.
/// No soporta subconsultas correlacionadas (se documenta como fuera de alcance).
#[allow(dead_code)] // API interna histórica; el executor usa `_ctx`
pub(crate) fn materialize_subqueries(
    database: &mut Database,
    expression: &Expr,
    snapshot: &Snapshot,
) -> Result<Expr, RuscaError> {
    materialize_subqueries_ctx(database, expression, snapshot, &BTreeMap::new())
}

/// Igual que [`materialize_subqueries`], con CTEs visibles por nombre.
pub(crate) fn materialize_subqueries_ctx(
    database: &mut Database,
    expression: &Expr,
    snapshot: &Snapshot,
    ctes: &BTreeMap<String, VirtualTable>,
) -> Result<Expr, RuscaError> {
    match expression {
        Expr::InSubquery {
            expr,
            query,
            negated,
        } => {
            let rows = execute_select_at_ctx(database, query, snapshot, ctes)?;
            let list = rows
                .iter()
                .filter_map(|row| row.values().next().cloned())
                .map(|value| scalar_expr(&value))
                .collect();
            Ok(Expr::InList {
                expr: Box::new(materialize_subqueries_ctx(database, expr, snapshot, ctes)?),
                list,
                negated: *negated,
            })
        }
        Expr::ScalarSubquery(query) => {
            let rows = execute_select_at_ctx(database, query, snapshot, ctes)?;
            scalar_from_rows(&rows)
        }
        Expr::Exists { query, negated } => {
            let rows = execute_select_at_ctx(database, query, snapshot, ctes)?;
            Ok(Expr::Bool(!rows.is_empty() != *negated))
        }
        Expr::And(left, right) => Ok(Expr::And(
            Box::new(materialize_subqueries_ctx(database, left, snapshot, ctes)?),
            Box::new(materialize_subqueries_ctx(database, right, snapshot, ctes)?),
        )),
        Expr::Or(left, right) => Ok(Expr::Or(
            Box::new(materialize_subqueries_ctx(database, left, snapshot, ctes)?),
            Box::new(materialize_subqueries_ctx(database, right, snapshot, ctes)?),
        )),
        Expr::Not(inner) => Ok(Expr::Not(Box::new(materialize_subqueries_ctx(
            database, inner, snapshot, ctes,
        )?))),
        Expr::Compare { left, op, right } => Ok(Expr::Compare {
            left: Box::new(materialize_subqueries_ctx(database, left, snapshot, ctes)?),
            op: *op,
            right: Box::new(materialize_subqueries_ctx(database, right, snapshot, ctes)?),
        }),
        Expr::InList {
            expr,
            list,
            negated,
        } => Ok(Expr::InList {
            expr: Box::new(materialize_subqueries_ctx(database, expr, snapshot, ctes)?),
            list: list
                .iter()
                .map(|item| materialize_subqueries_ctx(database, item, snapshot, ctes))
                .collect::<Result<_, _>>()?,
            negated: *negated,
        }),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => Ok(Expr::Between {
            expr: Box::new(materialize_subqueries_ctx(database, expr, snapshot, ctes)?),
            low: Box::new(materialize_subqueries_ctx(database, low, snapshot, ctes)?),
            high: Box::new(materialize_subqueries_ctx(database, high, snapshot, ctes)?),
            negated: *negated,
        }),
        Expr::Like {
            expr,
            pattern,
            case_insensitive,
            negated,
        } => Ok(Expr::Like {
            expr: Box::new(materialize_subqueries_ctx(database, expr, snapshot, ctes)?),
            pattern: Box::new(materialize_subqueries_ctx(
                database, pattern, snapshot, ctes,
            )?),
            case_insensitive: *case_insensitive,
            negated: *negated,
        }),
        other => Ok(other.clone()),
    }
}

/// Convierte el resultado de una subconsulta escalar en literal.
fn scalar_from_rows(rows: &[Row]) -> Result<Expr, RuscaError> {
    if rows.len() > 1 {
        return Err(RuscaError::TypeMismatch {
            message: format!(
                "subconsulta escalar devolvio {} filas (maximo 1)",
                rows.len()
            ),
        });
    }
    let Some(row) = rows.first() else {
        return Ok(Expr::Null);
    };
    if row.len() != 1 {
        return Err(RuscaError::TypeMismatch {
            message: format!(
                "subconsulta escalar debe proyectar 1 columna, proyecto {}",
                row.len()
            ),
        });
    }
    Ok(row.values().next().map(scalar_expr).unwrap_or(Expr::Null))
}

/// Convierte un escalar en literal del IR.
pub(crate) fn scalar_expr(value: &ScalarValue) -> Expr {
    match value {
        ScalarValue::Null => Expr::Null,
        ScalarValue::Bool(flag) => Expr::Bool(*flag),
        ScalarValue::Int(number) => Expr::Int(*number),
        ScalarValue::UInt(number) => Expr::Int(*number as i64),
        ScalarValue::Float(number) => Expr::Float(*number),
        ScalarValue::Text(text) => Expr::Text(text.clone()),
        ScalarValue::TimestampMillis(millis) => Expr::Int(*millis),
        ScalarValue::Bytes(bytes) => Expr::Text(format!("{bytes:?}")),
    }
}
