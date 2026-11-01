//! Evaluacion de expresiones, predicados y `CASE`.

use super::*;

/// Convierte un literal del IR a `ScalarValue`.
pub(crate) fn expr_literal(expression: &Expr) -> Option<ScalarValue> {
    match expression {
        Expr::Int(number) => Some(ScalarValue::Int(*number)),
        Expr::Float(number) => Some(ScalarValue::Float(*number)),
        Expr::Text(text) => Some(ScalarValue::Text(text.clone())),
        Expr::Null => Some(ScalarValue::Null),
        Expr::Bool(flag) => Some(ScalarValue::Bool(*flag)),
        Expr::Column(_)
        | Expr::Compare { .. }
        | Expr::And(_, _)
        | Expr::Or(_, _)
        | Expr::Not(_)
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Like { .. }
        | Expr::Coalesce(_)
        | Expr::Nullif(_, _)
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
    Ok(eval_truth(expression, record)? == Truth::True)
}

/// Valor de verdad SQL de tres valores (Kleene).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
    /// Verdadero.
    True,
    /// Falso.
    False,
    /// `NULL`/desconocido.
    Unknown,
}

impl Truth {
    /// Negacion Kleene.
    pub(crate) fn negate(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }

    /// Conjuncion Kleene.
    pub(crate) fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    /// Disyuncion Kleene.
    pub(crate) fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }
}

/// Convierte un escalar a verdad SQL (`NULL` -> Unknown).
fn scalar_truth(value: &ScalarValue) -> Result<Truth, RuscaError> {
    match value {
        ScalarValue::Bool(true) => Ok(Truth::True),
        ScalarValue::Bool(false) => Ok(Truth::False),
        ScalarValue::Null => Ok(Truth::Unknown),
        other => Err(RuscaError::TypeMismatch {
            message: format!("se esperaba BOOL, se obtuvo {other:?}"),
        }),
    }
}

/// Convierte verdad SQL a escalar (`Unknown` -> `NULL`).
fn truth_scalar(truth: Truth) -> ScalarValue {
    match truth {
        Truth::True => ScalarValue::Bool(true),
        Truth::False => ScalarValue::Bool(false),
        Truth::Unknown => ScalarValue::Null,
    }
}
/// Comprueba que cada columna del filtro exista en el esquema.
pub(crate) fn validate_filter_columns(
    table: &TableDef,
    expression: &Expr,
) -> Result<(), RuscaError> {
    match expression {
        Expr::Column(name) => table.column_type(name).map(|_| ()),
        Expr::Int(_) | Expr::Float(_) | Expr::Text(_) | Expr::Null | Expr::Bool(_) => Ok(()),
        Expr::Compare { left, right, .. } => {
            validate_filter_columns(table, left)?;
            validate_filter_columns(table, right)
        }
        Expr::And(left, right) | Expr::Or(left, right) => {
            validate_filter_columns(table, left)?;
            validate_filter_columns(table, right)
        }
        Expr::Not(inner) => validate_filter_columns(table, inner),
        Expr::IsNull { expr, .. } => validate_filter_columns(table, expr),
        Expr::InList { expr, list, .. } => {
            validate_filter_columns(table, expr)?;
            for item in list {
                validate_filter_columns(table, item)?;
            }
            Ok(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            validate_filter_columns(table, expr)?;
            validate_filter_columns(table, low)?;
            validate_filter_columns(table, high)
        }
        Expr::Like { expr, pattern, .. } => {
            validate_filter_columns(table, expr)?;
            validate_filter_columns(table, pattern)
        }
        Expr::Coalesce(items) => {
            for item in items {
                validate_filter_columns(table, item)?;
            }
            Ok(())
        }
        Expr::Nullif(left, right) => {
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
/// Evalúa un predicado con lógica SQL de tres valores.
pub(crate) fn eval_truth(expression: &Expr, record: &Record) -> Result<Truth, RuscaError> {
    match expression {
        Expr::And(left, right) => {
            let first = eval_truth(left, record)?;
            if first == Truth::False {
                return Ok(Truth::False);
            }
            Ok(first.and(eval_truth(right, record)?))
        }
        Expr::Or(left, right) => {
            let first = eval_truth(left, record)?;
            if first == Truth::True {
                return Ok(Truth::True);
            }
            Ok(first.or(eval_truth(right, record)?))
        }
        Expr::Not(inner) => Ok(eval_truth(inner, record)?.negate()),
        Expr::IsNull { expr, negated } => {
            let is_null = eval_operand(expr, record)? == ScalarValue::Null;
            Ok(if is_null != *negated {
                Truth::True
            } else {
                Truth::False
            })
        }
        Expr::Compare { left, op, right } => {
            let first = eval_operand(left, record)?;
            let second = eval_operand(right, record)?;
            compare_values(*op, &first, &second)
        }
        Expr::DocContains { json, .. } => Ok(if doc_contains(record, json)? {
            Truth::True
        } else {
            Truth::False
        }),
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let value = eval_operand(expr, record)?;
            let mut unknown = value == ScalarValue::Null;
            let mut found = false;
            for item in list {
                let candidate = eval_operand(item, record)?;
                match compare_values(CompareOp::Eq, &value, &candidate)? {
                    Truth::True => {
                        found = true;
                        break;
                    }
                    Truth::Unknown => unknown = true,
                    Truth::False => {}
                }
            }
            let truth = if found {
                Truth::True
            } else if unknown {
                Truth::Unknown
            } else {
                Truth::False
            };
            Ok(if *negated { truth.negate() } else { truth })
        }
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let value = eval_operand(expr, record)?;
            let lower = eval_operand(low, record)?;
            let upper = eval_operand(high, record)?;
            let truth = compare_values(CompareOp::GtEq, &value, &lower)?.and(compare_values(
                CompareOp::LtEq,
                &value,
                &upper,
            )?);
            Ok(if *negated { truth.negate() } else { truth })
        }
        Expr::Like {
            expr,
            pattern,
            case_insensitive,
            negated,
        } => {
            let value = eval_operand(expr, record)?;
            let pattern = eval_operand(pattern, record)?;
            if value == ScalarValue::Null || pattern == ScalarValue::Null {
                return Ok(Truth::Unknown);
            }
            let (ScalarValue::Text(value), ScalarValue::Text(pattern)) = (&value, &pattern) else {
                return Err(RuscaError::TypeMismatch {
                    message: "LIKE exige operandos TEXT".to_string(),
                });
            };
            let matched = like_match(value, pattern, *case_insensitive);
            let truth = if matched { Truth::True } else { Truth::False };
            Ok(if *negated { truth.negate() } else { truth })
        }
        Expr::Coalesce(_) | Expr::Nullif(_, _) => {
            scalar_truth(&eval_scalar(expression, &record.scalars)?)
        }
        Expr::Bool(flag) => Ok(if *flag { Truth::True } else { Truth::False }),
        Expr::Case { .. } => scalar_truth(&eval_scalar(expression, &record.scalars)?),
        Expr::Column(name) => match record.scalars.get(name) {
            Some(value) => scalar_truth(value),
            None => Ok(Truth::Unknown),
        },
        Expr::Null => Ok(Truth::Unknown),
        Expr::Int(_) | Expr::Float(_) | Expr::Text(_) => Err(RuscaError::TypeMismatch {
            message: "el filtro WHERE debe ser una comparación, un booleano o AND/OR/NOT"
                .to_string(),
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
        Expr::Null => Ok(ScalarValue::Null),
        Expr::Bool(flag) => Ok(ScalarValue::Bool(*flag)),
        Expr::Case { .. } => eval_scalar(expression, &record.scalars),
        Expr::Compare { left, op, right } => {
            let first = eval_operand(left, record)?;
            let second = eval_operand(right, record)?;
            Ok(truth_scalar(compare_values(*op, &first, &second)?))
        }
        Expr::And(_, _)
        | Expr::Or(_, _)
        | Expr::Not(_)
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Like { .. } => Ok(truth_scalar(eval_truth(expression, record)?)),
        Expr::Coalesce(_) | Expr::Nullif(_, _) => eval_scalar(expression, &record.scalars),
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
        Expr::Null => Ok(ScalarValue::Null),
        Expr::Bool(flag) => Ok(ScalarValue::Bool(*flag)),
        Expr::Compare { left, op, right } => {
            let first = eval_scalar(left, scalars)?;
            let second = eval_scalar(right, scalars)?;
            Ok(truth_scalar(compare_values(*op, &first, &second)?))
        }
        Expr::And(left, right) => {
            let first = scalar_truth(&eval_scalar(left, scalars)?)?;
            if first == Truth::False {
                return Ok(ScalarValue::Bool(false));
            }
            let second = scalar_truth(&eval_scalar(right, scalars)?)?;
            Ok(truth_scalar(first.and(second)))
        }
        Expr::Or(left, right) => {
            let first = scalar_truth(&eval_scalar(left, scalars)?)?;
            if first == Truth::True {
                return Ok(ScalarValue::Bool(true));
            }
            let second = scalar_truth(&eval_scalar(right, scalars)?)?;
            Ok(truth_scalar(first.or(second)))
        }
        Expr::Not(inner) => Ok(truth_scalar(
            scalar_truth(&eval_scalar(inner, scalars)?)?.negate(),
        )),
        Expr::IsNull { expr, negated } => {
            let is_null = eval_scalar(expr, scalars)? == ScalarValue::Null;
            Ok(ScalarValue::Bool(is_null != *negated))
        }
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let value = eval_scalar(expr, scalars)?;
            let mut unknown = value == ScalarValue::Null;
            let mut found = false;
            for item in list {
                let candidate = eval_scalar(item, scalars)?;
                match compare_values(CompareOp::Eq, &value, &candidate)? {
                    Truth::True => {
                        found = true;
                        break;
                    }
                    Truth::Unknown => unknown = true,
                    Truth::False => {}
                }
            }
            let truth = if found {
                Truth::True
            } else if unknown {
                Truth::Unknown
            } else {
                Truth::False
            };
            Ok(truth_scalar(if *negated { truth.negate() } else { truth }))
        }
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let value = eval_scalar(expr, scalars)?;
            let lower = eval_scalar(low, scalars)?;
            let upper = eval_scalar(high, scalars)?;
            let truth = compare_values(CompareOp::GtEq, &value, &lower)?.and(compare_values(
                CompareOp::LtEq,
                &value,
                &upper,
            )?);
            Ok(truth_scalar(if *negated { truth.negate() } else { truth }))
        }
        Expr::Like {
            expr,
            pattern,
            case_insensitive,
            negated,
        } => {
            let value = eval_scalar(expr, scalars)?;
            let pattern = eval_scalar(pattern, scalars)?;
            if value == ScalarValue::Null || pattern == ScalarValue::Null {
                return Ok(ScalarValue::Null);
            }
            let (ScalarValue::Text(value), ScalarValue::Text(pattern)) = (&value, &pattern) else {
                return Err(RuscaError::TypeMismatch {
                    message: "LIKE exige operandos TEXT".to_string(),
                });
            };
            let matched = like_match(value, pattern, *case_insensitive);
            let truth = if matched { Truth::True } else { Truth::False };
            Ok(truth_scalar(if *negated { truth.negate() } else { truth }))
        }
        Expr::Coalesce(items) => {
            for item in items {
                let value = eval_scalar(item, scalars)?;
                if value != ScalarValue::Null {
                    return Ok(value);
                }
            }
            Ok(ScalarValue::Null)
        }
        Expr::Nullif(left, right) => {
            let first = eval_scalar(left, scalars)?;
            if first == ScalarValue::Null {
                return Ok(ScalarValue::Null);
            }
            let second = eval_scalar(right, scalars)?;
            if second != ScalarValue::Null
                && compare_values(CompareOp::Eq, &first, &second)? == Truth::True
            {
                Ok(ScalarValue::Null)
            } else {
                Ok(first)
            }
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
    Ok(scalar_truth(&eval_scalar(expression, scalars)?)? == Truth::True)
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
                compare_values(CompareOp::Eq, operand_value, &candidate)? == Truth::True
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
/// Coincidencia `LIKE` con `%` (cualquier secuencia) y `_` (un caracter).
///
/// Programacion dinamica `O(m·n)` en tiempo y memoria, con texto/patron en
/// `Vec<char>` para Unicode. `case_insensitive` usa minúsculas Unicode.
fn like_match(text: &str, pattern: &str, case_insensitive: bool) -> bool {
    let (text, pattern) = if case_insensitive {
        (text.to_lowercase(), pattern.to_lowercase())
    } else {
        (text.to_string(), pattern.to_string())
    };
    let text: Vec<char> = text.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    let mut dp = vec![vec![false; text.len() + 1]; pattern.len() + 1];
    dp[0][0] = true;
    for row in 1..=pattern.len() {
        if pattern[row - 1] == '%' {
            dp[row][0] = dp[row - 1][0];
        }
    }
    for row in 1..=pattern.len() {
        for column in 1..=text.len() {
            dp[row][column] = match pattern[row - 1] {
                '%' => dp[row - 1][column] || dp[row][column - 1],
                '_' => dp[row - 1][column - 1],
                expected => dp[row - 1][column - 1] && expected == text[column - 1],
            };
        }
    }
    dp[pattern.len()][text.len()]
}

/// Compara dos valores (`NULL` excluye la fila; `Int↔Float` con coerción).
pub(crate) fn compare_values(
    operation: CompareOp,
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<Truth, RuscaError> {
    if *left == ScalarValue::Null || *right == ScalarValue::Null {
        return Ok(Truth::Unknown);
    }
    let ordering = order_values(left, right)?;
    let truth = match operation {
        CompareOp::Eq => ordering == Ordering::Equal,
        CompareOp::NotEq => ordering != Ordering::Equal,
        CompareOp::Lt => ordering == Ordering::Less,
        CompareOp::LtEq => ordering != Ordering::Greater,
        CompareOp::Gt => ordering == Ordering::Greater,
        CompareOp::GtEq => ordering != Ordering::Less,
    };
    Ok(if truth { Truth::True } else { Truth::False })
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

#[cfg(test)]
mod truth_tests {
    use super::Truth;

    /// Tablas de Kleene completas (matan mutantes de AND/OR/NOT).
    #[test]
    fn kleene_truth_tables_are_exact() {
        use Truth::{False, True, Unknown};
        let values = [True, False, Unknown];

        let and_expected = [
            [True, False, Unknown],
            [False, False, False],
            [Unknown, False, Unknown],
        ];
        let or_expected = [
            [True, True, True],
            [True, False, Unknown],
            [True, Unknown, Unknown],
        ];
        for (row, left) in values.iter().enumerate() {
            for (column, right) in values.iter().enumerate() {
                assert_eq!(left.and(*right), and_expected[row][column], "AND");
                assert_eq!(left.or(*right), or_expected[row][column], "OR");
            }
            let negated = match left {
                True => False,
                False => True,
                Unknown => Unknown,
            };
            assert_eq!(left.negate(), negated, "NOT");
        }
    }
}
