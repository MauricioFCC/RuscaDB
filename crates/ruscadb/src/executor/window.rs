//! Evaluación de funciones de ventana `OVER (...)` (SPEC-0165).
//!
//! El cálculo es materializado: se particiona, se ordena cada partición y se
//! evalúa la función por fila. Complejidad `O(n log n)` por ventana (el sort de
//! cada partición) y `O(n·w)` con `w` el tamaño del marco en los agregados.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use ruscadb_query::{FrameBound, FrameKind, OrderBy, WindowFunc, WindowItem};

use super::*;

/// Evalúa todas las ventanas de la proyección.
///
/// Args:
///     windows: Ventanas en el orden de la proyección.
///     records: Registros ya filtrados y ordenados.
///     table: Definición de la tabla (validación de columnas).
///
/// Returns:
///     Un vector de valores por ventana, alineado con `records`.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna no existe;
///     [`RuscaError::TypeMismatch`] si los valores no son comparables.
pub(crate) fn evaluate_windows(
    windows: &[WindowItem],
    records: &[Record],
    table: &TableDef,
) -> Result<Vec<Vec<ScalarValue>>, RuscaError> {
    windows
        .iter()
        .map(|window| evaluate_window(window, records, table))
        .collect()
}

/// Evalúa una ventana y devuelve un valor por registro.
fn evaluate_window(
    window: &WindowItem,
    records: &[Record],
    table: &TableDef,
) -> Result<Vec<ScalarValue>, RuscaError> {
    if let Some(arg) = &window.arg {
        table.column_type(arg)?;
    }
    for column in &window.over.partition_by {
        table.column_type(column)?;
    }
    if let Some(order_by) = &window.over.order_by {
        table.column_type(&order_by.column)?;
    }
    let mut values = vec![ScalarValue::Null; records.len()];
    let mut partitions: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        partitions
            .entry(partition_key(&window.over.partition_by, &record.scalars))
            .or_default()
            .push(index);
    }
    for indices in partitions.values_mut() {
        if let Some(order_by) = &window.over.order_by {
            let mut failure: Option<RuscaError> = None;
            indices.sort_by(|left, right| {
                if failure.is_some() {
                    return Ordering::Equal;
                }
                match compare_order_values(
                    records[*left].scalars.get(&order_by.column),
                    records[*right].scalars.get(&order_by.column),
                    order_by.desc,
                ) {
                    Ok(ordering) => ordering,
                    Err(error) => {
                        failure = Some(error);
                        Ordering::Equal
                    }
                }
            });
            if let Some(error) = failure {
                return Err(error);
            }
        }
        let partition: Vec<&Record> = indices.iter().map(|index| &records[*index]).collect();
        for (position, index) in indices.iter().enumerate() {
            values[*index] = compute_window(window, &partition, position)?;
        }
    }
    Ok(values)
}

/// Clave canónica de partición (reutiliza la codificación de filas).
fn partition_key(columns: &[String], scalars: &ScalarMap) -> Vec<u8> {
    let mut key = Row::new();
    for column in columns {
        key.insert(
            column.clone(),
            scalars.get(column).cloned().unwrap_or(ScalarValue::Null),
        );
    }
    row_key(&key)
}

/// Calcula el valor de la ventana para la fila `position` de su partición.
fn compute_window(
    window: &WindowItem,
    partition: &[&Record],
    position: usize,
) -> Result<ScalarValue, RuscaError> {
    let length = partition.len();
    let range = frame_range(window, position, length);
    match window.func {
        WindowFunc::RowNumber => Ok(ScalarValue::Int((position + 1) as i64)),
        WindowFunc::Rank => Ok(ScalarValue::Int(rank(window, partition, position)? as i64)),
        WindowFunc::DenseRank => Ok(ScalarValue::Int(
            dense_rank(window, partition, position)? as i64
        )),
        WindowFunc::Ntile => {
            let buckets = window.offset.unwrap_or(1).max(1) as usize;
            let bucket = (position * buckets) / length + 1;
            Ok(ScalarValue::Int(bucket.min(buckets) as i64))
        }
        WindowFunc::Lag => {
            let offset = window.offset.unwrap_or(1) as usize;
            Ok(position
                .checked_sub(offset)
                .map_or(ScalarValue::Null, |index| {
                    argument(partition, index, window)
                }))
        }
        WindowFunc::Lead => {
            let offset = window.offset.unwrap_or(1) as usize;
            Ok(partition
                .get(position + offset)
                .map_or(ScalarValue::Null, |_| {
                    argument(partition, position + offset, window)
                }))
        }
        WindowFunc::FirstValue => range.map_or(Ok(ScalarValue::Null), |(start, _)| {
            Ok(argument(partition, start, window))
        }),
        WindowFunc::LastValue => range.map_or(Ok(ScalarValue::Null), |(_, end)| {
            Ok(argument(partition, end, window))
        }),
        WindowFunc::NthValue => {
            let nth = window.offset.unwrap_or(1) as usize;
            match range {
                Some((start, end)) if nth >= 1 && start + nth - 1 <= end => {
                    Ok(argument(partition, start + nth - 1, window))
                }
                _ => Ok(ScalarValue::Null),
            }
        }
        WindowFunc::Sum
        | WindowFunc::Avg
        | WindowFunc::Count
        | WindowFunc::Min
        | WindowFunc::Max => {
            let Some((start, end)) = range else {
                return Ok(ScalarValue::Null);
            };
            Ok(aggregate_window(window, &partition[start..=end]))
        }
    }
}

/// Rango `[start, end]` del marco dentro de la partición (`None` si vacío).
fn frame_range(window: &WindowItem, position: usize, length: usize) -> Option<(usize, usize)> {
    let Some(frame) = &window.over.frame else {
        return Some((0, length.saturating_sub(1)));
    };
    if frame.kind == FrameKind::Range || length == 0 {
        // RANGE se evalúa como la partición completa (documentado).
        return Some((0, length.saturating_sub(1)));
    }
    let start = bound_index(frame.start, position, length);
    let end = bound_index(frame.end, position, length);
    if start > end {
        None
    } else {
        Some((start, end))
    }
}

/// Índice de un límite de marco.
fn bound_index(bound: FrameBound, position: usize, length: usize) -> usize {
    match bound {
        FrameBound::UnboundedPreceding => 0,
        FrameBound::Preceding(value) => position.saturating_sub(value as usize),
        FrameBound::CurrentRow => position,
        FrameBound::Following(value) => (position + value as usize).min(length.saturating_sub(1)),
        FrameBound::UnboundedFollowing => length.saturating_sub(1),
    }
}

/// Valor de la columna de la ventana en una fila de la partición.
fn argument(partition: &[&Record], index: usize, window: &WindowItem) -> ScalarValue {
    let Some(column) = &window.arg else {
        return ScalarValue::Null;
    };
    partition
        .get(index)
        .and_then(|record| record.scalars.get(column).cloned())
        .unwrap_or(ScalarValue::Null)
}

/// Rango con huecos (`RANK`) de la fila `position`.
fn rank(window: &WindowItem, partition: &[&Record], position: usize) -> Result<usize, RuscaError> {
    let Some(order_by) = &window.over.order_by else {
        return Ok(1);
    };
    let mut rank = 1;
    for index in 0..position {
        if !same_order_key(partition, index, position, order_by)? {
            rank = index + 2;
        }
    }
    Ok(rank)
}

/// Rango sin huecos (`DENSE_RANK`) de la fila `position`.
fn dense_rank(
    window: &WindowItem,
    partition: &[&Record],
    position: usize,
) -> Result<usize, RuscaError> {
    let Some(order_by) = &window.over.order_by else {
        return Ok(1);
    };
    let mut rank = 1;
    for index in 1..=position {
        if !same_order_key(partition, index - 1, index, order_by)? {
            rank += 1;
        }
    }
    Ok(rank)
}

/// Indica si dos posiciones tienen la misma clave de orden.
fn same_order_key(
    partition: &[&Record],
    left: usize,
    right: usize,
    order_by: &OrderBy,
) -> Result<bool, RuscaError> {
    Ok(compare_order_values(
        partition[left].scalars.get(&order_by.column),
        partition[right].scalars.get(&order_by.column),
        order_by.desc,
    )? == Ordering::Equal)
}

/// Agrega el marco de una ventana.
fn aggregate_window(window: &WindowItem, frame: &[&Record]) -> ScalarValue {
    match window.func {
        WindowFunc::Count => {
            let count = frame
                .iter()
                .filter(|record| {
                    window.arg.as_ref().is_none_or(|column| {
                        !matches!(record.scalars.get(column), None | Some(ScalarValue::Null))
                    })
                })
                .count();
            ScalarValue::Int(count as i64)
        }
        WindowFunc::Sum => sum_window(window, frame),
        WindowFunc::Avg => {
            let values: Vec<f64> = frame
                .iter()
                .filter_map(|record| {
                    window
                        .arg
                        .as_ref()
                        .and_then(|column| record.scalars.get(column))
                        .and_then(as_f64)
                })
                .collect();
            if values.is_empty() {
                ScalarValue::Null
            } else {
                ScalarValue::Float(values.iter().sum::<f64>() / values.len() as f64)
            }
        }
        WindowFunc::Min => extreme_window(window, frame, false),
        WindowFunc::Max => extreme_window(window, frame, true),
        _ => ScalarValue::Null,
    }
}

/// Suma del marco conservando el tipo entero cuando todos los valores lo son.
fn sum_window(window: &WindowItem, frame: &[&Record]) -> ScalarValue {
    let Some(column) = &window.arg else {
        return ScalarValue::Null;
    };
    let mut integer_total: i64 = 0;
    let mut float_total: f64 = 0.0;
    let mut is_float = false;
    let mut seen = false;
    for record in frame {
        match record.scalars.get(column) {
            Some(ScalarValue::Int(value)) => {
                seen = true;
                integer_total = integer_total.saturating_add(*value);
                float_total += *value as f64;
            }
            Some(ScalarValue::UInt(value)) => {
                seen = true;
                integer_total = integer_total.saturating_add(*value as i64);
                float_total += *value as f64;
            }
            Some(ScalarValue::Float(value)) => {
                seen = true;
                is_float = true;
                float_total += value;
            }
            _ => {}
        }
    }
    if !seen {
        ScalarValue::Null
    } else if is_float {
        ScalarValue::Float(float_total)
    } else {
        ScalarValue::Int(integer_total)
    }
}

/// Mínimo/máximo del marco (ignora `NULL`).
fn extreme_window(window: &WindowItem, frame: &[&Record], maximum: bool) -> ScalarValue {
    let Some(column) = &window.arg else {
        return ScalarValue::Null;
    };
    let mut best: Option<ScalarValue> = None;
    for record in frame {
        let Some(value) = record.scalars.get(column) else {
            continue;
        };
        if matches!(value, ScalarValue::Null) {
            continue;
        }
        best = match best {
            None => Some(value.clone()),
            Some(current) => {
                let ordering = compare_order_values(Some(value), Some(&current), false)
                    .unwrap_or(Ordering::Equal);
                let replace = if maximum {
                    ordering == Ordering::Greater
                } else {
                    ordering == Ordering::Less
                };
                Some(if replace { value.clone() } else { current })
            }
        };
    }
    best.unwrap_or(ScalarValue::Null)
}

/// Convierte un escalar numérico a `f64`.
fn as_f64(value: &ScalarValue) -> Option<f64> {
    match value {
        ScalarValue::Int(number) => Some(*number as f64),
        ScalarValue::UInt(number) => Some(*number as f64),
        ScalarValue::Float(number) => Some(*number),
        _ => None,
    }
}
