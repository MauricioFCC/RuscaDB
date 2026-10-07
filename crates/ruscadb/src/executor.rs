//! Planificador y ejecutor `SELECT` (SPEC-0012, FR-0012-04/05).
//!
//! [`plan_for`] elige `IndexScan` cuando el filtro contiene `columna =
//! literal` sobre la columna indexada; en otro caso `FullScan`.
//! [`execute_select`] evalúa filtro (con coerción numérica `Int↔Float`),
//! proyección y `LIMIT`. `NULL` excluye la fila y los tipos incompatibles
//! devuelven [`RuscaError::TypeMismatch`].

use std::cmp::Ordering;
use std::collections::BTreeMap;

use ruscadb_core::{Record, RuscaError, ScalarMap, ScalarValue};
use ruscadb_query::{CompareOp, Expr, Projection, Select};

use crate::catalog::{Catalog, TableDef};
use crate::database::Database;
use crate::heap::{heap_read, heap_scan};
use crate::index::index_lookup_eq;

/// Fila resultado: escalares proyectados por nombre de columna.
pub type Row = BTreeMap<String, ScalarValue>;

/// Plan de acceso a una tabla.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Recorrido completo del heap.
    FullScan,
    /// Búsqueda de igualdad en el índice de la columna.
    IndexScan {
        /// Columna indexada usada.
        column: String,
    },
}

/// Elige el plan de acceso para un `SELECT`.
///
/// Args:
///     select: Consulta analizada.
///     catalog: Catálogo con las tablas e índices.
///
/// Returns:
///     `IndexScan` si el filtro contiene `columna = literal` sobre la
///     columna indexada; `FullScan` en otro caso.
///
/// Errors:
///     [`RuscaError::TableNotFound`] si la tabla no existe.
pub fn plan_for(select: &Select, catalog: &Catalog) -> Result<Plan, RuscaError> {
    let table = catalog.get(&select.from)?;
    let indexed = table.index.as_ref().map(|index| index.column.clone());
    let Some(column) = indexed else {
        return Ok(Plan::FullScan);
    };
    if find_eq_literal(select.filter.as_ref(), &column).is_some() {
        return Ok(Plan::IndexScan { column });
    }
    Ok(Plan::FullScan)
}

/// Ejecuta un `SELECT` analizado (planifica y evalúa).
///
/// Args:
///     database: Base abierta.
///     select: Consulta analizada.
///
/// Returns:
///     Filas proyectadas (hasta `LIMIT`, en orden de inserción).
///
/// Errors:
///     [`RuscaError::TableNotFound`] / [`RuscaError::ColumnNotFound`] /
///     [`RuscaError::TypeMismatch`] ante errores de esquema.
pub fn execute_select(database: &mut Database, select: &Select) -> Result<Vec<Row>, RuscaError> {
    let catalog = Catalog::load(database)?;
    let plan = plan_for(select, &catalog)?;
    execute_with_plan(database, select, plan)
}

/// Ejecuta un `SELECT` con un plan fijado (equivalencia Full/IndexScan).
///
/// Args:
///     database: Base abierta.
///     select: Consulta analizada.
///     plan: Plan de acceso a usar.
///
/// Returns:
///     Filas proyectadas (hasta `LIMIT`, en orden de inserción).
pub fn execute_with_plan(
    database: &mut Database,
    select: &Select,
    plan: Plan,
) -> Result<Vec<Row>, RuscaError> {
    let table = Catalog::load(database)?.get(&select.from)?.clone();
    let candidates = fetch_candidates(database, &table, &plan, select.filter.as_ref())?;
    let mut rows = Vec::new();
    for (_, record) in &candidates {
        if keeps_row(&table, select.filter.as_ref(), &record.scalars)? {
            rows.push(project_row(&table, &select.projection, &record.scalars)?);
        }
        if let Some(limit) = select.limit {
            if rows.len() >= limit as usize {
                break;
            }
        }
    }
    Ok(rows)
}

/// Obtiene los registros candidatos según el plan.
fn fetch_candidates(
    database: &mut Database,
    table: &TableDef,
    plan: &Plan,
    filter: Option<&Expr>,
) -> Result<Vec<(crate::heap::RowLocator, Record)>, RuscaError> {
    match plan {
        Plan::FullScan => heap_scan(database, table),
        Plan::IndexScan { column } => match find_eq_literal(filter, column) {
            Some(literal) => fetch_by_index(database, table, &literal),
            None => heap_scan(database, table),
        },
    }
}

/// Lee por índice los registros que igualan el literal.
fn fetch_by_index(
    database: &mut Database,
    table: &TableDef,
    literal: &ScalarValue,
) -> Result<Vec<(crate::heap::RowLocator, Record)>, RuscaError> {
    let mut rows = Vec::new();
    for locator in index_lookup_eq(database, table, literal)? {
        rows.push((locator, heap_read(database, locator)?));
    }
    Ok(rows)
}

/// Busca `columna = literal` en una conjunción `AND` (ambos órdenes).
fn find_eq_literal(filter: Option<&Expr>, column: &str) -> Option<ScalarValue> {
    let expression = filter?;
    match expression {
        Expr::And(left, right) => {
            find_eq_literal(Some(left), column).or_else(|| find_eq_literal(Some(right), column))
        }
        Expr::Compare { left, op, right } if *op == CompareOp::Eq => {
            eq_literal_sides(left, right, column)
        }
        _ => None,
    }
}

/// Extrae el literal si un lado es la columna y el otro un literal.
fn eq_literal_sides(left: &Expr, right: &Expr, column: &str) -> Option<ScalarValue> {
    if let Expr::Column(name) = left {
        if name == column {
            return expr_literal(right);
        }
    }
    if let Expr::Column(name) = right {
        if name == column {
            return expr_literal(left);
        }
    }
    None
}

/// Convierte un literal del IR a `ScalarValue`.
fn expr_literal(expression: &Expr) -> Option<ScalarValue> {
    match expression {
        Expr::Int(number) => Some(ScalarValue::Int(*number)),
        Expr::Float(number) => Some(ScalarValue::Float(*number)),
        Expr::Text(text) => Some(ScalarValue::Text(text.clone())),
        Expr::Column(_) | Expr::Compare { .. } | Expr::And(_, _) => None,
    }
}

/// Decide si la fila pasa el filtro (valida las columnas contra el esquema).
fn keeps_row(
    table: &TableDef,
    filter: Option<&Expr>,
    scalars: &ScalarMap,
) -> Result<bool, RuscaError> {
    let Some(expression) = filter else {
        return Ok(true);
    };
    validate_filter_columns(table, expression)?;
    eval_predicate(expression, scalars)
}

/// Comprueba que cada columna del filtro exista en el esquema.
fn validate_filter_columns(table: &TableDef, expression: &Expr) -> Result<(), RuscaError> {
    match expression {
        Expr::Column(name) => table.column_type(name).map(|_| ()),
        Expr::Int(_) | Expr::Float(_) | Expr::Text(_) => Ok(()),
        Expr::Compare { left, right, .. } => {
            validate_filter_columns(table, left)?;
            validate_filter_columns(table, right)
        }
        Expr::And(left, right) => {
            validate_filter_columns(table, left)?;
            validate_filter_columns(table, right)
        }
    }
}

/// Evalúa un predicado sobre los escalares de la fila.
fn eval_predicate(expression: &Expr, scalars: &ScalarMap) -> Result<bool, RuscaError> {
    match expression {
        Expr::And(left, right) => {
            Ok(eval_predicate(left, scalars)? && eval_predicate(right, scalars)?)
        }
        Expr::Compare { left, op, right } => {
            let first = eval_operand(left, scalars)?;
            let second = eval_operand(right, scalars)?;
            compare_values(*op, &first, &second)
        }
        Expr::Column(_) | Expr::Int(_) | Expr::Float(_) | Expr::Text(_) => {
            Err(RuscaError::TypeMismatch {
                message: "el filtro WHERE debe ser una comparación o AND de comparaciones"
                    .to_string(),
            })
        }
    }
}

/// Evalúa un operando (columna → valor de la fila, ausente → `NULL`).
fn eval_operand(expression: &Expr, scalars: &ScalarMap) -> Result<ScalarValue, RuscaError> {
    match expression {
        Expr::Column(name) => Ok(scalars.get(name).cloned().unwrap_or(ScalarValue::Null)),
        Expr::Int(number) => Ok(ScalarValue::Int(*number)),
        Expr::Float(number) => Ok(ScalarValue::Float(*number)),
        Expr::Text(text) => Ok(ScalarValue::Text(text.clone())),
        Expr::Compare { .. } | Expr::And(_, _) => Err(RuscaError::TypeMismatch {
            message: "una comparación no puede anidarse como operando".to_string(),
        }),
    }
}

/// Compara dos valores (`NULL` excluye la fila; `Int↔Float` con coerción).
fn compare_values(
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
fn order_values(left: &ScalarValue, right: &ScalarValue) -> Result<Ordering, RuscaError> {
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
fn compare_floats(first: f64, second: f64) -> Ordering {
    first.partial_cmp(&second).unwrap_or(Ordering::Less)
}

/// Proyecta las columnas pedidas (valida el esquema primero).
fn project_row(
    table: &TableDef,
    projection: &Projection,
    scalars: &ScalarMap,
) -> Result<Row, RuscaError> {
    match projection {
        Projection::All => Ok(scalars.clone()),
        Projection::Columns(columns) => {
            let mut row = Row::new();
            for column in columns {
                table.column_type(column)?;
                let value = scalars.get(column).cloned().unwrap_or(ScalarValue::Null);
                row.insert(column.clone(), value);
            }
            Ok(row)
        }
    }
}
