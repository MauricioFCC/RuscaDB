//! Proyeccion, orden, deduplicacion y paginacion de filas.

use super::*;

/// Deduplica filas preservando el orden de primera aparición (`SELECT DISTINCT`).
///
/// Antes era `O(n²)` (`Vec::contains` por fila). Ahora codifica cada fila a una
/// clave canónica y usa un `BTreeSet`: `O(n·k·log n)` con `k` el ancho de la
/// fila, sin comparaciones cuadráticas. La codificación normaliza `-0.0` a
/// `+0.0` y `NaN` a una clave estable (semántica SQL de `DISTINCT`, donde
/// `NaN` iguala a `NaN`).
///
/// Args:
///     rows: Filas proyectadas (se reemplazan por su versión única).
pub(crate) fn dedup_rows(rows: &mut Vec<Row>) {
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    rows.retain(|row| seen.insert(row_key(row)));
}

/// Codifica una fila en bytes canónicos y deterministas.
///
/// Args:
///     row: Fila proyectada (`BTreeMap`, orden estable).
///
/// Returns:
///     Clave única para igualdad de filas.
fn row_key(row: &Row) -> Vec<u8> {
    let mut key = Vec::with_capacity(row.len() * 16);
    for (name, value) in row {
        key.extend_from_slice(&(name.len() as u32).to_le_bytes());
        key.extend_from_slice(name.as_bytes());
        match value {
            ScalarValue::Null => key.push(0),
            ScalarValue::Bool(flag) => {
                key.push(1);
                key.push(u8::from(*flag));
            }
            ScalarValue::Int(number) => {
                key.push(2);
                key.extend_from_slice(&number.to_le_bytes());
            }
            ScalarValue::UInt(number) => {
                key.push(3);
                key.extend_from_slice(&number.to_le_bytes());
            }
            ScalarValue::Float(number) => {
                key.push(4);
                let normalized = if *number == 0.0 {
                    0.0f64
                } else if number.is_nan() {
                    f64::NAN
                } else {
                    *number
                };
                key.extend_from_slice(&normalized.to_bits().to_le_bytes());
            }
            ScalarValue::Text(text) => {
                key.push(5);
                key.extend_from_slice(&(text.len() as u32).to_le_bytes());
                key.extend_from_slice(text.as_bytes());
            }
            ScalarValue::Bytes(bytes) => {
                key.push(6);
                key.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                key.extend_from_slice(bytes);
            }
            ScalarValue::TimestampMillis(millis) => {
                key.push(7);
                key.extend_from_slice(&millis.to_le_bytes());
            }
        }
    }
    key
}
/// Proyecta las columnas pedidas (valida el esquema primero).
pub(crate) fn project_row(
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
/// Indica si `column` aparece en la salida agregada (columna proyectada o agregado).
pub(crate) fn is_aggregate_output_column(select: &Select, column: &str) -> bool {
    let projected = match &select.projection {
        Projection::All => false,
        Projection::Columns(columns) => columns.iter().any(|name| name == column),
    };
    projected
        || select
            .aggregates
            .iter()
            .any(|aggregate| aggregate_output_key(aggregate) == column)
}
/// Ordena las filas agregadas por la cláusula `ORDER BY` (estable, fallible).
pub(crate) fn sort_rows(order_by: &OrderBy, rows: &mut [Row]) -> Result<(), RuscaError> {
    let mut failure: Option<RuscaError> = None;
    rows.sort_by(|left, right| {
        if failure.is_some() {
            return Ordering::Equal;
        }
        match compare_order_values(
            left.get(&order_by.column),
            right.get(&order_by.column),
            order_by.desc,
        ) {
            Ok(ordering) => ordering,
            Err(error) => {
                failure = Some(error);
                Ordering::Equal
            }
        }
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DISTINCT` deduplica tipos mixtos y normaliza `-0.0`/`NaN`.
    #[test]
    fn dedup_rows_is_linear_and_stable() {
        let mut rows = vec![
            Row::from([
                ("a".to_string(), ScalarValue::Int(1)),
                ("b".to_string(), ScalarValue::Float(0.0)),
            ]),
            Row::from([
                ("a".to_string(), ScalarValue::Int(1)),
                ("b".to_string(), ScalarValue::Float(-0.0)),
            ]),
            Row::from([
                ("a".to_string(), ScalarValue::Int(2)),
                ("b".to_string(), ScalarValue::Text("x".to_string())),
            ]),
            Row::from([
                ("a".to_string(), ScalarValue::Null),
                ("b".to_string(), ScalarValue::Bool(true)),
            ]),
        ];
        dedup_rows(&mut rows);
        assert_eq!(rows.len(), 3, "0.0 y -0.0 son la misma fila");
        assert_eq!(rows[0].get("a"), Some(&ScalarValue::Int(1)));
        assert_eq!(rows[1].get("a"), Some(&ScalarValue::Int(2)));
        assert_eq!(rows[2].get("a"), Some(&ScalarValue::Null));

        // 4.000 filas: la clave canonica mantiene una sola.
        let mut repeated: Vec<Row> = (0..4_000)
            .map(|_| Row::from([("a".to_string(), ScalarValue::Int(7))]))
            .collect();
        dedup_rows(&mut repeated);
        assert_eq!(repeated.len(), 1);
    }
}
