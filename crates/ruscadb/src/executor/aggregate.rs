//! Agregacion por hash, `GROUP BY`, `HAVING` y agregados con `CASE`.

use super::*;

/// Indica si la consulta requiere agregación (`GROUP BY` o agregados).
///
/// Args:
///     select: Consulta analizada.
///
/// Returns:
///     `true` si hay columnas de agrupación o agregados en la proyección.
pub(crate) fn is_aggregate_query(select: &Select) -> bool {
    !select.group_by.is_empty() || !select.aggregates.is_empty()
}
/// Tipo de acumulador resuelto de un agregado (columna y tipo ya verificados).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AggKind {
    /// `COUNT(*)`.
    CountStar,
    /// `COUNT(columna)`.
    Count,
    /// `SUM` sobre columna entera.
    SumInt,
    /// `SUM` sobre columna flotante.
    SumFloat,
    /// `AVG`.
    Avg,
    /// `MIN`.
    Min,
    /// `MAX`.
    Max,
}
/// Estado acumulado de un grupo: claves representativas + celdas de agregados.
pub(crate) struct GroupAccumulator {
    /// Valores de las columnas de `GROUP BY` (de la primera fila del grupo).
    keys: ScalarMap,
    /// Número de filas del grupo (`COUNT(*)`).
    row_count: u64,
    /// Estado por agregado, alineado con `select.aggregates`.
    cells: Vec<AggregateState>,
}
impl GroupAccumulator {
    /// Crea un grupo vacío (sin filas) para la agregación global.
    pub(crate) fn empty(kinds: &[AggKind]) -> Self {
        Self {
            keys: ScalarMap::new(),
            row_count: 0,
            cells: new_cells(kinds),
        }
    }

    /// Crea un grupo con las claves de su primera fila.
    pub(crate) fn new(keys: ScalarMap, kinds: &[AggKind]) -> Self {
        Self {
            keys,
            row_count: 0,
            cells: new_cells(kinds),
        }
    }

    /// Materializa la fila de salida: columnas de grupo + agregados.
    pub(crate) fn to_row(&self, projection: &Projection, aggregates: &[Aggregate]) -> Row {
        let mut row = Row::new();
        if let Projection::Columns(columns) = projection {
            for column in columns {
                let value = self.keys.get(column).cloned().unwrap_or(ScalarValue::Null);
                row.insert(column.clone(), value);
            }
        }
        for (index, aggregate) in aggregates.iter().enumerate() {
            let value = self.aggregate_value(aggregates, index);
            row.insert(aggregate_output_key(aggregate), value);
        }
        row
    }

    /// Valor final del agregado `aggregates[index]` en este grupo.
    ///
    /// Args:
    ///     aggregates: Lista de agregados alineada con `self.cells`.
    ///     index: Posición del agregado cuyo valor se pide.
    ///
    /// Returns:
    ///     El valor agregado (`COUNT(*)` usa el conteo de filas).
    pub(crate) fn aggregate_value(&self, aggregates: &[Aggregate], index: usize) -> ScalarValue {
        if aggregates[index].func == AggFunc::CountStar {
            ScalarValue::Int(self.row_count as i64)
        } else {
            self.cells[index].finish()
        }
    }
}
/// Estado acumulado de un único agregado en un grupo.
pub(crate) enum AggregateState {
    /// `COUNT(*)` (se resuelve con el conteo de filas del grupo).
    CountStar,
    /// `COUNT(columna)`.
    Count {
        /// Valores no `NULL` vistos.
        count: u64,
    },
    /// `SUM` entero.
    SumInt {
        /// Suma acumulada.
        total: i64,
        /// `true` si se vio algún valor no `NULL`.
        seen: bool,
    },
    /// `SUM` flotante.
    SumFloat {
        /// Suma acumulada.
        total: f64,
        /// `true` si se vio algún valor no `NULL`.
        seen: bool,
    },
    /// `AVG`.
    Avg {
        /// Suma acumulada.
        total: f64,
        /// Número de valores no `NULL`.
        count: u64,
    },
    /// `MIN`/`MAX` (el orden lo decide el tipo de agregado).
    Extreme {
        /// Extremo actual (`None` si aún no hay valores no `NULL`).
        current: Option<ScalarValue>,
    },
}
impl AggregateState {
    /// Crea el estado inicial para un tipo de agregado.
    pub(crate) fn new(kind: AggKind) -> Self {
        match kind {
            AggKind::CountStar => Self::CountStar,
            AggKind::Count => Self::Count { count: 0 },
            AggKind::SumInt => Self::SumInt {
                total: 0,
                seen: false,
            },
            AggKind::SumFloat => Self::SumFloat {
                total: 0.0,
                seen: false,
            },
            AggKind::Avg => Self::Avg {
                total: 0.0,
                count: 0,
            },
            AggKind::Min | AggKind::Max => Self::Extreme { current: None },
        }
    }

    /// Valor final del agregado (semántica SQL de `NULL`).
    pub(crate) fn finish(&self) -> ScalarValue {
        match self {
            Self::CountStar => ScalarValue::Null,
            Self::Count { count } => ScalarValue::Int(*count as i64),
            Self::SumInt { total, seen } => {
                if *seen {
                    ScalarValue::Int(*total)
                } else {
                    ScalarValue::Null
                }
            }
            Self::SumFloat { total, seen } => {
                if *seen {
                    ScalarValue::Float(*total)
                } else {
                    ScalarValue::Null
                }
            }
            Self::Avg { total, count } => {
                if *count == 0 {
                    ScalarValue::Null
                } else {
                    ScalarValue::Float(*total / *count as f64)
                }
            }
            Self::Extreme { current } => current.clone().unwrap_or(ScalarValue::Null),
        }
    }
}
/// Crea las celdas de agregado alineadas con la lista de tipos.
pub(crate) fn new_cells(kinds: &[AggKind]) -> Vec<AggregateState> {
    kinds
        .iter()
        .map(|kind| AggregateState::new(*kind))
        .collect()
}
/// Ejecuta la agregación por hash (*hash aggregation*; patrón DuckDB/DataFusion).
///
/// Agrupa por las columnas de `GROUP BY` con una `BTreeMap` de clave canónica
/// (determinismo del orden de salida) y acumula los agregados en una sola
/// pasada. Sin `GROUP BY` produce una única fila global (incluso con 0 filas).
/// La semántica SQL de `NULL` se respeta: `COUNT(*)` cuenta filas mientras que
/// `COUNT(col)`/`SUM`/`AVG`/`MIN`/`MAX` ignoran los `NULL`.
///
/// Args:
///     table: Definición de la tabla (esquema).
///     select: Consulta con agregados y/o `GROUP BY`.
///     records: Filas ya filtradas (WHERE/MATCH/KNN/TRAVERSE) y visibles a MVCC.
///
/// Returns:
///     Una fila por grupo (o una global), con columnas de grupo + agregados.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna no existe;
///     [`RuscaError::TypeMismatch`] si una columna proyectada no está agrupada
///     o si `SUM`/`AVG` se aplican sobre una columna no numérica.
pub(crate) fn aggregate_records(
    table: &TableDef,
    select: &Select,
    records: &[Record],
) -> Result<Vec<Row>, RuscaError> {
    validate_aggregate_query(table, select)?;
    // Agregados del SELECT más los que solo aparecen en el HAVING (SPEC-0051):
    // se acumulan todos, pero `to_row` solo emite los del SELECT.
    let mut all_aggregates: Vec<Aggregate> = select.aggregates.clone();
    all_aggregates.extend(having_aggregates(select));
    let kinds: Vec<AggKind> = all_aggregates
        .iter()
        .map(|aggregate| aggregate_kind(table, aggregate))
        .collect::<Result<_, _>>()?;
    let mut groups: BTreeMap<Vec<u8>, GroupAccumulator> = BTreeMap::new();
    if select.group_by.is_empty() {
        groups.insert(Vec::new(), GroupAccumulator::empty(&kinds));
    }
    for record in records {
        let keys = group_values(&select.group_by, &record.scalars);
        let key = group_key(table, &select.group_by, &keys)?;
        let group = groups
            .entry(key)
            .or_insert_with(|| GroupAccumulator::new(keys, &kinds));
        group.row_count += 1;
        accumulate(&all_aggregates, &kinds, &record.scalars, &mut group.cells)?;
    }
    let mut rows: Vec<Row> = groups
        .values()
        .filter(|group| group_satisfies_having(group, &all_aggregates, &select.having))
        .map(|group| group.to_row(&select.projection, &select.aggregates))
        .collect();
    if select.distinct {
        dedup_rows(&mut rows);
    }
    if let Some(order_by) = &select.order_by {
        sort_rows(order_by, &mut rows)?;
    }
    let offset = select.offset.unwrap_or(0) as usize;
    if offset > 0 {
        rows.drain(0..offset.min(rows.len()));
    }
    if let Some(limit) = select.limit {
        rows.truncate(limit as usize);
    }
    Ok(rows)
}
/// Agregados que solo aparecen en el `HAVING` (no están en el `SELECT`).
///
/// Args:
///     select: Consulta con agregados y condiciones `HAVING`.
///
/// Returns:
///     Un [`Aggregate`] sin alias por cada `(func, columna)` del `HAVING`
///     ausente en `select.aggregates` (para acumularlo junto al resto).
pub(crate) fn having_aggregates(select: &Select) -> Vec<Aggregate> {
    let mut extra = Vec::new();
    for condition in &select.having {
        let present = select.aggregates.iter().any(|aggregate| {
            aggregate.func == condition.func && aggregate.column == condition.column
        });
        if !present {
            extra.push(Aggregate {
                func: condition.func,
                column: condition.column.clone(),
                alias: None,
                expr: None,
            });
        }
    }
    extra
}
/// Indica si un grupo satisface todas las condiciones del `HAVING` (AND).
///
/// Un literal `NULL` o un agregado `NULL` excluyen al grupo (como en `WHERE`).
/// Un agregado del `HAVING` ausente en la lista acumulada es un error interno.
pub(crate) fn group_satisfies_having(
    group: &GroupAccumulator,
    aggregates: &[Aggregate],
    having: &[HavingCondition],
) -> bool {
    having.iter().all(|condition| {
        let position = aggregates.iter().position(|aggregate| {
            aggregate.func == condition.func && aggregate.column == condition.column
        });
        let Some(index) = position else {
            return false;
        };
        let value = group.aggregate_value(aggregates, index);
        let Ok(literal) = having_literal_value(&condition.literal) else {
            return false;
        };
        matches!(
            compare_values(condition.op, &value, &literal),
            Ok(Truth::True)
        )
    })
}
/// Valor escalar de un literal del `HAVING` (el parser solo admite Int/Float/Text).
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si el literal no es un escalar simple.
pub(crate) fn having_literal_value(literal: &Expr) -> Result<ScalarValue, RuscaError> {
    match literal {
        Expr::Int(number) => Ok(ScalarValue::Int(*number)),
        Expr::Float(number) => Ok(ScalarValue::Float(*number)),
        Expr::Text(text) => Ok(ScalarValue::Text(text.clone())),
        other => Err(RuscaError::TypeMismatch {
            message: format!("HAVING exige un literal, se obtuvo {other:?}"),
        }),
    }
}
/// Valida la consulta agregada (proyección agrupada, columnas y tipos).
///
/// Args:
///     table: Definición de la tabla.
///     select: Consulta con agregados y/o `GROUP BY`.
///
/// Returns:
///     `Ok(())` si la consulta es agregable.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna no existe;
///     [`RuscaError::TypeMismatch`] si la proyección viola la regla de
///     agrupación o si un agregado no es aplicable al tipo de la columna.
pub(crate) fn validate_aggregate_query(
    table: &TableDef,
    select: &Select,
) -> Result<(), RuscaError> {
    let Projection::Columns(columns) = &select.projection else {
        return Err(RuscaError::TypeMismatch {
            message: "SELECT * no se puede combinar con GROUP BY ni agregados".to_string(),
        });
    };
    for column in &select.group_by {
        table.column_type(column)?;
    }
    for column in columns {
        table.column_type(column)?;
        if !select.group_by.contains(column) {
            return Err(RuscaError::TypeMismatch {
                message: format!(
                    "la columna '{column}' no está en GROUP BY ni es un agregado (agrúpala con GROUP BY o envuélvela en COUNT/SUM/AVG/MIN/MAX)"
                ),
            });
        }
    }
    let _ = aggregate_kinds(table, select)?;
    // Valida los agregados del HAVING con las mismas reglas (columna
    // existente, SUM/AVG numéricos) para errores accionables (SPEC-0051).
    for aggregate in having_aggregates(select) {
        let _ = aggregate_kind(table, &aggregate)?;
    }
    if let Some(order_by) = &select.order_by
        && !is_aggregate_output_column(select, &order_by.column)
    {
        return Err(RuscaError::ColumnNotFound {
            column: order_by.column.clone(),
        });
    }
    Ok(())
}
/// Resuelve el tipo de acumulador de cada agregado (valida columnas/tipos).
pub(crate) fn aggregate_kinds(
    table: &TableDef,
    select: &Select,
) -> Result<Vec<AggKind>, RuscaError> {
    select
        .aggregates
        .iter()
        .map(|aggregate| aggregate_kind(table, aggregate))
        .collect()
}
/// Resuelve el tipo de acumulador de un agregado.
pub(crate) fn aggregate_kind(
    table: &TableDef,
    aggregate: &Aggregate,
) -> Result<AggKind, RuscaError> {
    if aggregate.func == AggFunc::CountStar {
        return Ok(AggKind::CountStar);
    }
    let column_type = match (&aggregate.expr, &aggregate.column) {
        (Some(expr), _) => infer_expr_type(table, expr)?,
        (None, Some(column)) => table.column_type(column)?,
        (None, None) => {
            return Err(RuscaError::TypeMismatch {
                message: format!("{} requiere una columna o CASE", aggregate.func.as_str()),
            });
        }
    };
    match aggregate.func {
        AggFunc::CountStar => Ok(AggKind::CountStar),
        AggFunc::Count => Ok(AggKind::Count),
        AggFunc::Min => Ok(AggKind::Min),
        AggFunc::Max => Ok(AggKind::Max),
        AggFunc::Sum | AggFunc::Avg => numeric_kind(
            aggregate.func,
            aggregate.column.as_deref().unwrap_or("CASE"),
            column_type,
        ),
    }
}
/// Infiere el tipo de una expresión escalar (para resolver `SUM`/`AVG`).
///
/// Args:
///     table: Esquema de la tabla.
///     expression: Expresión del IR.
///
/// Returns:
///     El [`ColumnType`] resultante.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna no existe;
///     [`RuscaError::TypeMismatch`] si las ramas de `CASE` no son compatibles.
pub(crate) fn infer_expr_type(
    table: &TableDef,
    expression: &Expr,
) -> Result<ColumnType, RuscaError> {
    match expression {
        Expr::Column(name) => table.column_type(name),
        Expr::Int(_) => Ok(ColumnType::Int),
        Expr::Float(_) => Ok(ColumnType::Float),
        Expr::Text(_) | Expr::Null => Ok(ColumnType::Text),
        Expr::Bool(_)
        | Expr::Compare { .. }
        | Expr::And(_, _)
        | Expr::Or(_, _)
        | Expr::Not(_)
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Like { .. }
        | Expr::Match { .. } => Ok(ColumnType::Bool),
        Expr::Coalesce(items) => {
            let mut result = None;
            for item in items {
                if matches!(item, Expr::Null) {
                    continue;
                }
                result = Some(unify_types(result, infer_expr_type(table, item)?)?);
            }
            result.ok_or_else(|| RuscaError::TypeMismatch {
                message: "COALESCE sin argumentos no tiene tipo".to_string(),
            })
        }
        Expr::Nullif(left, _) => infer_expr_type(table, left),
        Expr::DocExtract { .. } | Expr::DocContains { .. } => Ok(ColumnType::Text),
        Expr::Case {
            branches,
            otherwise,
            ..
        } => {
            let mut result: Option<ColumnType> = None;
            for (_, value) in branches {
                if matches!(value, Expr::Null) {
                    continue;
                }
                result = Some(unify_types(result, infer_expr_type(table, value)?)?);
            }
            if let Some(otherwise) = otherwise
                && !matches!(otherwise.as_ref(), Expr::Null)
            {
                result = Some(unify_types(result, infer_expr_type(table, otherwise)?)?);
            }
            result.ok_or_else(|| RuscaError::TypeMismatch {
                message: "CASE sin ramas THEN no tiene tipo".to_string(),
            })
        }
    }
}
/// Unifica dos tipos de ramas de `CASE` (`Int`+`Float` → `Float`).
pub(crate) fn unify_types(
    current: Option<ColumnType>,
    next: ColumnType,
) -> Result<ColumnType, RuscaError> {
    let Some(current) = current else {
        return Ok(next);
    };
    match (current, next) {
        (ColumnType::Int, ColumnType::Float) | (ColumnType::Float, ColumnType::Int) => {
            Ok(ColumnType::Float)
        }
        (left, right) if left == right => Ok(left),
        (left, right) => Err(RuscaError::TypeMismatch {
            message: format!("ramas de CASE incompatibles: {left:?} y {right:?}"),
        }),
    }
}
/// Resuelve `SUM`/`AVG` exigiendo una columna numérica.
pub(crate) fn numeric_kind(
    func: AggFunc,
    column: &str,
    column_type: ColumnType,
) -> Result<AggKind, RuscaError> {
    match (func, column_type) {
        (AggFunc::Sum, ColumnType::Int) => Ok(AggKind::SumInt),
        (AggFunc::Sum, ColumnType::Float) => Ok(AggKind::SumFloat),
        (AggFunc::Avg, ColumnType::Int | ColumnType::Float) => Ok(AggKind::Avg),
        _ => Err(RuscaError::TypeMismatch {
            message: format!(
                "{} requiere una columna numérica, pero '{column}' es {column_type:?}",
                func.as_str()
            ),
        }),
    }
}
/// Extrae los valores de las columnas de `GROUP BY` (ausente → `NULL`).
pub(crate) fn group_values(group_by: &[String], scalars: &ScalarMap) -> ScalarMap {
    group_by
        .iter()
        .map(|column| {
            let value = scalars.get(column).cloned().unwrap_or(ScalarValue::Null);
            (column.clone(), value)
        })
        .collect()
}
/// Construye la clave canónica ordenable de un grupo (`NULL` ordena primero).
pub(crate) fn group_key(
    table: &TableDef,
    group_by: &[String],
    keys: &ScalarMap,
) -> Result<Vec<u8>, RuscaError> {
    let mut encoded = Vec::new();
    for column in group_by {
        match keys.get(column).unwrap_or(&ScalarValue::Null) {
            ScalarValue::Null => encoded.push(0x00),
            other => encoded.extend_from_slice(&canonical_key(table.column_type(column)?, other)?),
        }
        encoded.push(0xFF);
    }
    Ok(encoded)
}
/// Acumula una fila en las celdas de su grupo (ignorando `NULL`).
pub(crate) fn accumulate(
    aggregates: &[Aggregate],
    kinds: &[AggKind],
    scalars: &ScalarMap,
    cells: &mut [AggregateState],
) -> Result<(), RuscaError> {
    for ((aggregate, kind), state) in aggregates.iter().zip(kinds).zip(cells.iter_mut()) {
        let value = match (&aggregate.expr, aggregate.column.as_deref()) {
            (Some(expr), _) => {
                let scalar = eval_scalar(expr, scalars)?;
                (scalar != ScalarValue::Null).then_some(scalar)
            }
            (None, Some(column)) => scalars
                .get(column)
                .filter(|value| **value != ScalarValue::Null)
                .cloned(),
            (None, None) => None,
        };
        apply_aggregate(*kind, value.as_ref(), state)?;
    }
    Ok(())
}
/// Aplica un valor al estado del agregado correspondiente.
pub(crate) fn apply_aggregate(
    kind: AggKind,
    value: Option<&ScalarValue>,
    state: &mut AggregateState,
) -> Result<(), RuscaError> {
    match (kind, state) {
        (AggKind::CountStar, AggregateState::CountStar) => {}
        (AggKind::Count, AggregateState::Count { count }) => {
            if value.is_some() {
                *count += 1;
            }
        }
        (AggKind::SumInt, AggregateState::SumInt { total, seen }) => {
            if let Some(value) = value {
                *total += numeric_i64(value)?;
                *seen = true;
            }
        }
        (AggKind::SumFloat, AggregateState::SumFloat { total, seen }) => {
            if let Some(value) = value {
                *total += numeric_f64(value)?;
                *seen = true;
            }
        }
        (AggKind::Avg, AggregateState::Avg { total, count }) => {
            if let Some(value) = value {
                *total += numeric_f64(value)?;
                *count += 1;
            }
        }
        (AggKind::Min, AggregateState::Extreme { current }) => {
            update_extreme(current, value, false)?;
        }
        (AggKind::Max, AggregateState::Extreme { current }) => {
            update_extreme(current, value, true)?;
        }
        _ => {}
    }
    Ok(())
}
/// Actualiza el extremo actual con un candidato no nulo (`MIN` o `MAX`).
pub(crate) fn update_extreme(
    current: &mut Option<ScalarValue>,
    candidate: Option<&ScalarValue>,
    want_max: bool,
) -> Result<(), RuscaError> {
    let Some(candidate) = candidate else {
        return Ok(());
    };
    match current {
        None => *current = Some(candidate.clone()),
        Some(existing) => {
            let ordering = order_values(candidate, existing)?;
            let replace = if want_max {
                ordering == Ordering::Greater
            } else {
                ordering == Ordering::Less
            };
            if replace {
                *current = Some(candidate.clone());
            }
        }
    }
    Ok(())
}
/// Convierte un valor entero para `SUM` entero.
pub(crate) fn numeric_i64(value: &ScalarValue) -> Result<i64, RuscaError> {
    match value {
        ScalarValue::Int(number) => Ok(*number),
        other => Err(RuscaError::TypeMismatch {
            message: format!("SUM sobre una columna no entera: {other:?}"),
        }),
    }
}
/// Convierte un valor numérico a `f64` para `SUM`/`AVG`.
pub(crate) fn numeric_f64(value: &ScalarValue) -> Result<f64, RuscaError> {
    match value {
        ScalarValue::Int(number) => Ok(*number as f64),
        ScalarValue::Float(number) => Ok(*number),
        ScalarValue::UInt(number) => Ok(*number as f64),
        other => Err(RuscaError::TypeMismatch {
            message: format!("se esperaba un valor numérico para SUM/AVG, se obtuvo {other:?}"),
        }),
    }
}
/// Nombre de salida de un agregado: el alias si existe, o uno derivado.
pub(crate) fn aggregate_output_key(aggregate: &Aggregate) -> String {
    if let Some(alias) = &aggregate.alias {
        return alias.clone();
    }
    let func = aggregate.func.as_str().to_ascii_lowercase();
    if aggregate.func == AggFunc::CountStar {
        return "count".to_string();
    }
    if let Some(expr) = &aggregate.expr {
        let sanitized: String = expr
            .to_string()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        return format!("{func}_{sanitized}");
    }
    let column = aggregate.column.as_deref().unwrap_or_default();
    format!("{func}_{column}")
}
