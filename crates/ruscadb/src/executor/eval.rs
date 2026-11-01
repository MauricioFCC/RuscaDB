//! Evaluacion de expresiones, predicados y `CASE`.

use super::*;

/// Convierte un literal del IR a `ScalarValue`.
pub(crate) fn expr_literal(expression: &Expr) -> Option<ScalarValue> {
    match expression {
        Expr::Int(number) => Some(ScalarValue::Int(*number)),
        Expr::Float(number) => Some(ScalarValue::Float(*number)),
        Expr::Text(text) => Some(ScalarValue::Text(text.clone())),
        Expr::Bool(flag) => Some(ScalarValue::Bool(*flag)),
        Expr::Column(_)
        | Expr::Compare { .. }
        | Expr::And(_, _)
        | Expr::Case { .. }
        | Expr::Match { .. }
        | Expr::DocExtract { .. }
        | Expr::DocContains { .. } => None,
    }
}
/// Convierte un literal del IR a `ScalarValue` o falla con error accionable.
///
/// Args:
///     expression: Expresión del IR que debe ser un literal.
///
/// Returns:
///     El [`ScalarValue`] del literal.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si la expresión no es un literal escalar.
pub(crate) fn literal_scalar(expression: &Expr) -> Result<ScalarValue, RuscaError> {
    expr_literal(expression).ok_or_else(|| RuscaError::TypeMismatch {
        message: "se esperaba un literal (entero, flotante o texto)".to_string(),
    })
}
/// Decide si la fila pasa el filtro (valida las columnas contra el esquema).
pub(crate) fn keeps_row(
    table: &TableDef,
    filter: Option<&Expr>,
    record: &Record,
) -> Result<bool, RuscaError> {
    let Some(expression) = filter else {
        return Ok(true);
    };
    validate_filter_columns(table, expression)?;
    eval_predicate(expression, record)
}
/// Comprueba que cada columna del filtro exista en el esquema.
pub(crate) fn validate_filter_columns(
    table: &TableDef,
    expression: &Expr,
) -> Result<(), RuscaError> {
    match expression {
        Expr::Column(name) => table.column_type(name).map(|_| ()),
        Expr::Int(_) | Expr::Float(_) | Expr::Text(_) | Expr::Bool(_) => Ok(()),
        Expr::Compare { left, right, .. } => {
            validate_filter_columns(table, left)?;
            validate_filter_columns(table, right)
        }
        Expr::And(left, right) => {
            validate_filter_columns(table, left)?;
            validate_filter_columns(table, right)
        }
        Expr::Case {
            operand,
            branches,
            otherwise,
        } => {
            if let Some(operand) = operand {
                validate_filter_columns(table, operand)?;
            }
            for (condition, result) in branches {
                validate_filter_columns(table, condition)?;
                validate_filter_columns(table, result)?;
            }
            if let Some(otherwise) = otherwise {
                validate_filter_columns(table, otherwise)?;
            }
            Ok(())
        }
        // `MATCH` se extrae antes de evaluar el filtro escalar (lo resuelve el
        // índice full-text), así que aquí no valida columnas. Los operadores
        // documentales tampoco referencian columnas del esquema (usan `Record.doc`).
        Expr::Match { .. } | Expr::DocExtract { .. } | Expr::DocContains { .. } => Ok(()),
    }
}
/// Evalúa un predicado sobre el registro (escalares + documento).
pub(crate) fn eval_predicate(expression: &Expr, record: &Record) -> Result<bool, RuscaError> {
    match expression {
        Expr::And(left, right) => {
            Ok(eval_predicate(left, record)? && eval_predicate(right, record)?)
        }
        Expr::Compare { left, op, right } => {
            let first = eval_operand(left, record)?;
            let second = eval_operand(right, record)?;
            compare_values(*op, &first, &second)
        }
        Expr::DocContains { json, .. } => doc_contains(record, json),
        Expr::Bool(flag) => Ok(*flag),
        Expr::Case { .. } => match eval_scalar(expression, &record.scalars)? {
            ScalarValue::Bool(flag) => Ok(flag),
            other => Err(RuscaError::TypeMismatch {
                message: format!("CASE en WHERE debe producir BOOL, se obtuvo {other:?}"),
            }),
        },
        Expr::Column(name) => match record.scalars.get(name) {
            Some(ScalarValue::Bool(flag)) => Ok(*flag),
            Some(ScalarValue::Null) | None => Ok(false),
            Some(other) => Err(RuscaError::TypeMismatch {
                message: format!("la columna '{name}' debe ser BOOL en WHERE, es {other:?}"),
            }),
        },
        Expr::Int(_) | Expr::Float(_) | Expr::Text(_) => Err(RuscaError::TypeMismatch {
            message: "el filtro WHERE debe ser una comparación, un booleano o AND".to_string(),
        }),
        Expr::DocExtract { .. } => Err(RuscaError::TypeMismatch {
            message: "la extracción documental '->' debe usarse dentro de una comparación"
                .to_string(),
        }),
        Expr::Match { .. } => Err(RuscaError::TypeMismatch {
            message: "MATCH se resuelve en el índice full-text, no como comparación escalar"
                .to_string(),
        }),
    }
}
/// Evalúa un operando (columna o extracción documental → valor; ausente → `NULL`).
pub(crate) fn eval_operand(expression: &Expr, record: &Record) -> Result<ScalarValue, RuscaError> {
    match expression {
        Expr::Column(name) => Ok(record
            .scalars
            .get(name)
            .cloned()
            .unwrap_or(ScalarValue::Null)),
        Expr::DocExtract { path, .. } => {
            Ok(extract_doc_scalar(record, path).unwrap_or(ScalarValue::Null))
        }
        Expr::Int(number) => Ok(ScalarValue::Int(*number)),
        Expr::Float(number) => Ok(ScalarValue::Float(*number)),
        Expr::Text(text) => Ok(ScalarValue::Text(text.clone())),
        Expr::Bool(flag) => Ok(ScalarValue::Bool(*flag)),
        Expr::Case { .. } => eval_scalar(expression, &record.scalars),
        Expr::Compare { left, op, right } => {
            let first = eval_operand(left, record)?;
            let second = eval_operand(right, record)?;
            Ok(ScalarValue::Bool(compare_values(*op, &first, &second)?))
        }
        Expr::And(left, right) => Ok(ScalarValue::Bool(
            eval_predicate(left, record)? && eval_predicate(right, record)?,
        )),
        Expr::Match { .. } | Expr::DocContains { .. } => Err(RuscaError::TypeMismatch {
            message: "MATCH/@> no pueden anidarse como operando".to_string(),
        }),
    }
}
/// Evalúa una expresión escalar sobre las columnas de una fila.
///
/// Se usa dentro de agregados con `CASE` (SPEC-0182): soporta columnas,
/// literales, comparaciones, `AND` y `CASE` anidados; los operadores
/// documentales/full-text no aplican aquí.
///
/// Args:
///     expression: Expresión del IR.
///     scalars: Columnas de la fila.
///
/// Returns:
///     El valor escalar resultante (`NULL` si una columna no está presente).
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si la expresión no es escalar.
pub(crate) fn eval_scalar(
    expression: &Expr,
    scalars: &ScalarMap,
) -> Result<ScalarValue, RuscaError> {
    match expression {
        Expr::Column(name) => Ok(scalars.get(name).cloned().unwrap_or(ScalarValue::Null)),
        Expr::Int(number) => Ok(ScalarValue::Int(*number)),
        Expr::Float(number) => Ok(ScalarValue::Float(*number)),
        Expr::Text(text) => Ok(ScalarValue::Text(text.clone())),
        Expr::Bool(flag) => Ok(ScalarValue::Bool(*flag)),
        Expr::Compare { left, op, right } => {
            let first = eval_scalar(left, scalars)?;
            let second = eval_scalar(right, scalars)?;
            Ok(ScalarValue::Bool(compare_values(*op, &first, &second)?))
        }
        Expr::And(left, right) => {
            let first = eval_scalar_bool(left, scalars)?;
            let second = eval_scalar_bool(right, scalars)?;
            Ok(ScalarValue::Bool(first && second))
        }
        Expr::Case { .. } => eval_case(expression, scalars),
        Expr::Match { .. } | Expr::DocExtract { .. } | Expr::DocContains { .. } => {
            Err(RuscaError::TypeMismatch {
                message: "expresión no escalar dentro de CASE/agregado".to_string(),
            })
        }
    }
}
/// Evalúa una subexpresión exigiendo resultado booleano.
pub(crate) fn eval_scalar_bool(expression: &Expr, scalars: &ScalarMap) -> Result<bool, RuscaError> {
    match eval_scalar(expression, scalars)? {
        ScalarValue::Bool(flag) => Ok(flag),
        ScalarValue::Null => Ok(false),
        other => Err(RuscaError::TypeMismatch {
            message: format!("se esperaba BOOL en CASE, se obtuvo {other:?}"),
        }),
    }
}
/// Evalúa `CASE` (simple o buscado) sobre las columnas de una fila.
pub(crate) fn eval_case(expression: &Expr, scalars: &ScalarMap) -> Result<ScalarValue, RuscaError> {
    let Expr::Case {
        operand,
        branches,
        otherwise,
    } = expression
    else {
        return Err(RuscaError::TypeMismatch {
            message: "se esperaba CASE".to_string(),
        });
    };
    let operand_value = match operand {
        Some(operand) => Some(eval_scalar(operand, scalars)?),
        None => None,
    };
    for (condition, result) in branches {
        let matched = match &operand_value {
            Some(operand_value) => {
                let candidate = eval_scalar(condition, scalars)?;
                compare_values(CompareOp::Eq, operand_value, &candidate)?
            }
            None => eval_scalar_bool(condition, scalars)?,
        };
        if matched {
            return eval_scalar(result, scalars);
        }
    }
    match otherwise {
        Some(otherwise) => eval_scalar(otherwise, scalars),
        None => Ok(ScalarValue::Null),
    }
}
/// Compara dos valores (`NULL` excluye la fila; `Int↔Float` con coerción).
pub(crate) fn compare_values(
    operation: CompareOp,
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<bool, RuscaError> {
    if *left == ScalarValue::Null || *right == ScalarValue::Null {
        return Ok(false);
    }
    let ordering = order_values(left, right)?;
    Ok(match operation {
        CompareOp::Eq => ordering == Ordering::Equal,
        CompareOp::NotEq => ordering != Ordering::Equal,
        CompareOp::Lt => ordering == Ordering::Less,
        CompareOp::LtEq => ordering != Ordering::Greater,
        CompareOp::Gt => ordering == Ordering::Greater,
        CompareOp::GtEq => ordering != Ordering::Less,
    })
}
/// Ordena dos valores no nulos del mismo dominio.
pub(crate) fn order_values(
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<Ordering, RuscaError> {
    match (left, right) {
        (ScalarValue::Int(first), ScalarValue::Int(second)) => Ok(first.cmp(second)),
        (ScalarValue::Float(first), ScalarValue::Float(second)) => {
            Ok(compare_floats(*first, *second))
        }
        (ScalarValue::Int(number), ScalarValue::Float(other)) => {
            Ok(compare_floats(*number as f64, *other))
        }
        (ScalarValue::Float(other), ScalarValue::Int(number)) => {
            Ok(compare_floats(*other, *number as f64))
        }
        (ScalarValue::Text(first), ScalarValue::Text(second)) => Ok(first.cmp(second)),
        (ScalarValue::Bool(first), ScalarValue::Bool(second)) => Ok(first.cmp(second)),
        _ => Err(RuscaError::TypeMismatch {
            message: format!("no se puede comparar {left:?} con {right:?} en el filtro"),
        }),
    }
}
/// Compara flotantes (`NaN` nunca iguala: excluye la fila).
pub(crate) fn compare_floats(first: f64, second: f64) -> Ordering {
    first.partial_cmp(&second).unwrap_or(Ordering::Less)
}
