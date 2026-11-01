//! Proyeccion, orden, deduplicacion y paginacion de filas.

use super::*;

/// Deduplica filas preservando el orden de primera aparición (`SELECT DISTINCT`).
///
/// Compara filas completas (`Row` = mapa columna→escalar) con igualdad; el
/// dedupe es O(n²) sobre las filas proyectadas, acotadas por el `LIMIT`
/// implícito anti-DoS (SPEC-0054), y por eso no usa hashing (los `ScalarValue`
/// con `Float` no son `Hash`/`Ord`).
///
/// Args:
///     rows: Filas proyectadas (se reemplazan por su versión única).
pub(crate) fn dedup_rows(rows: &mut Vec<Row>) {
    let mut unique: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows.drain(..) {
        if !unique.contains(&row) {
            unique.push(row);
        }
    }
    *rows = unique;
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
