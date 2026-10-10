//! Ejecutor del `INNER JOIN` por igualdad (SPEC-0052, FR-0052-01/02/03).
//!
//! [`execute_join_at`] resuelve `SELECT ... FROM a JOIN b ON a.x = b.y` con
//! nested-loop sobre la tabla exterior y lookups puntuales en la interior:
//! usa el índice secundario ([`index_lookup_eq`](crate::index::index_lookup_eq))
//! cuando la columna interior está indexada y un barrido filtrado en caso
//! contrario. Ambas tablas aportan solo sus filas visibles para el snapshot
//! MVCC (los mismos candidatos visibles que el `SELECT` normal). Las filas
//! fusionadas usan columnas prefijadas `tabla.columna`.

use ruscadb_core::{Record, RuscaError, ScalarValue};
use ruscadb_query::{JoinClause, Projection, Select};
use ruscadb_txn::Snapshot;

use crate::catalog::TableDef;
use crate::database::Database;
use crate::executor::{Row, is_visible};
use crate::heap::{heap_read, heap_scan};
use crate::index::index_lookup_eq;

/// Lados resueltos del `ON`: exterior (`FROM`) e interior (`JOIN`).
struct JoinSides {
    /// Definición de la tabla exterior (`FROM`).
    outer: TableDef,
    /// Columna de igualdad en la tabla exterior.
    outer_column: String,
    /// Definición de la tabla interior (`JOIN`).
    inner: TableDef,
    /// Columna de igualdad en la tabla interior.
    inner_column: String,
}

/// Ejecuta un `SELECT` con `JOIN` y visibilidad "as of" `snapshot`.
///
/// Args:
///     database: Base abierta.
///     select: Consulta analizada (con `join` presente).
///     join: Cláusula `JOIN` ya parseada.
///     snapshot: Vista fija de visibilidad MVCC.
///
/// Returns:
///     Filas emparejadas y proyectadas (hasta `LIMIT`).
///
/// Errors:
///     [`RuscaError::TableNotFound`] / [`RuscaError::ColumnNotFound`] ante
///     errores de esquema; [`RuscaError::TypeMismatch`] si la consulta combina
///     el `JOIN` con cláusulas fuera de alcance o pide columnas ambiguas.
pub(crate) fn execute_join_at(
    database: &mut Database,
    select: &Select,
    join: &JoinClause,
    snapshot: &Snapshot,
) -> Result<Vec<Row>, RuscaError> {
    reject_unsupported(select)?;
    let sides = resolve_sides(database, select, join)?;
    let outer_rows = visible_records(database, &sides.outer, snapshot)?;
    let mut merged = Vec::new();
    if inner_is_indexed(&sides) {
        for outer in &outer_rows {
            let key = join_value(&sides.outer, &sides.outer_column, outer);
            for inner in &inner_matches_indexed(database, &sides, &key, snapshot)? {
                merged.push(merge_rows(&sides, outer, inner));
            }
        }
    } else {
        let inner_rows = visible_records(database, &sides.inner, snapshot)?;
        for outer in &outer_rows {
            let key = join_value(&sides.outer, &sides.outer_column, outer);
            for inner in inner_rows.iter().filter(|record| {
                keys_equal(&key, &join_value(&sides.inner, &sides.inner_column, record))
            }) {
                merged.push(merge_rows(&sides, outer, inner));
            }
        }
    }
    project_all(&sides, &merged, select)
}

/// Rechaza las cláusulas fuera del alcance de SPEC-0052.
///
/// Args:
///     select: Consulta analizada con `JOIN`.
///
/// Returns:
///     `Ok(())` si solo usa proyección y `LIMIT`.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si combina `JOIN` con `WHERE`, agregados,
///     `GROUP BY`, `ORDER BY`, `KNN` o `TRAVERSE`.
fn reject_unsupported(select: &Select) -> Result<(), RuscaError> {
    if select.filter.is_some() {
        return Err(join_unsupported("WHERE"));
    }
    if !select.aggregates.is_empty() || !select.group_by.is_empty() {
        return Err(join_unsupported("agregados o GROUP BY"));
    }
    if select.order_by.is_some() {
        return Err(join_unsupported("ORDER BY"));
    }
    if select.knn.is_some() || select.traverse.is_some() {
        return Err(join_unsupported("KNN o TRAVERSE"));
    }
    Ok(())
}

/// Construye el error de cláusula no soportada con `JOIN`.
///
/// Args:
///     clause: Cláusula que se intentó combinar con el `JOIN`.
///
/// Returns:
///     Un [`RuscaError::TypeMismatch`] accionable.
fn join_unsupported(clause: &str) -> RuscaError {
    RuscaError::TypeMismatch {
        message: format!(
            "{clause} no se admite con JOIN en SPEC-0052 (solo SELECT <cols|*> FROM a JOIN b ON a.x = b.y [LIMIT n])"
        ),
    }
}

/// Resuelve los lados del `ON` contra el catálogo (exterior = `FROM`).
///
/// Args:
///     database: Base abierta (para cargar el catálogo).
///     select: Consulta (aporta la tabla exterior `from`).
///     join: Cláusula `JOIN` (aporta la tabla interior y los lados del `ON`).
///
/// Returns:
///     Los [`JoinSides`] con ambas definiciones y columnas de igualdad.
///
/// Errors:
///     [`RuscaError::TableNotFound`] si un lado referencia una tabla ajena;
///     [`RuscaError::TypeMismatch`] si el `ON` no cruza ambas tablas;
///     [`RuscaError::ColumnNotFound`] si una columna no existe.
fn resolve_sides(
    database: &mut Database,
    select: &Select,
    join: &JoinClause,
) -> Result<JoinSides, RuscaError> {
    let catalog = crate::catalog::Catalog::load(database)?;
    let outer = catalog.get(&select.from)?.clone();
    let inner = catalog.get(&join.table)?.clone();
    for side in [&join.left, &join.right] {
        if side.table != select.from && side.table != join.table {
            return Err(RuscaError::TableNotFound {
                table: side.table.clone(),
            });
        }
    }
    let crossed = (join.left.table == select.from && join.right.table == join.table)
        || (join.right.table == select.from && join.left.table == join.table);
    if !crossed {
        return Err(RuscaError::TypeMismatch {
            message: format!(
                "el ON del JOIN debe cruzar ambas tablas ('{}.x = {}.y'), no '{}' con '{}'",
                select.from, join.table, join.left.table, join.right.table
            ),
        });
    }
    let (outer_column, inner_column) = if join.left.table == select.from {
        (join.left.column.clone(), join.right.column.clone())
    } else {
        (join.right.column.clone(), join.left.column.clone())
    };
    outer.column_type(&outer_column)?;
    inner.column_type(&inner_column)?;
    Ok(JoinSides {
        outer,
        outer_column,
        inner,
        inner_column,
    })
}

/// Indica si la columna interior tiene índice secundario (lookup puntual).
///
/// Args:
///     sides: Lados resueltos del `JOIN`.
///
/// Returns:
///     `true` si la tabla interior indexa su columna de igualdad.
fn inner_is_indexed(sides: &JoinSides) -> bool {
    sides
        .inner
        .index
        .as_ref()
        .is_some_and(|index| index.column == sides.inner_column)
}

/// Lee las filas visibles de una tabla para el snapshot (como el `SELECT`).
///
/// Args:
///     database: Base abierta.
///     table: Definición de la tabla.
///     snapshot: Vista fija de visibilidad MVCC.
///
/// Returns:
///     Los registros visibles, en orden de inserción.
fn visible_records(
    database: &mut Database,
    table: &TableDef,
    snapshot: &Snapshot,
) -> Result<Vec<Record>, RuscaError> {
    let mut records = Vec::new();
    for (_, record) in heap_scan(database, table)? {
        if is_visible(snapshot, &record) {
            records.push(record);
        }
    }
    Ok(records)
}

/// Extrae el valor de igualdad de un registro (ausente → `NULL`).
///
/// Args:
///     table: Definición de la tabla (no usada para validar; firma simétrica).
///     column: Columna de igualdad.
///     record: Registro origen.
///
/// Returns:
///     El escalar de la columna, o `NULL` si falta.
fn join_value(_table: &TableDef, column: &str, record: &Record) -> ScalarValue {
    record
        .scalars
        .get(column)
        .cloned()
        .unwrap_or(ScalarValue::Null)
}

/// Busca las filas interiores visibles que igualan `key` por índice.
///
/// Args:
///     database: Base abierta.
///     sides: Lados resueltos del `JOIN`.
///     key: Valor exterior (`NULL` nunca empareja).
///     snapshot: Vista fija de visibilidad MVCC.
///
/// Returns:
///     Los registros interiores visibles con clave igual (re-verificada).
fn inner_matches_indexed(
    database: &mut Database,
    sides: &JoinSides,
    key: &ScalarValue,
    snapshot: &Snapshot,
) -> Result<Vec<Record>, RuscaError> {
    if *key == ScalarValue::Null {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    for locator in index_lookup_eq(database, &sides.inner, key)? {
        let record = heap_read(database, locator)?;
        let candidate = join_value(&sides.inner, &sides.inner_column, &record);
        if is_visible(snapshot, &record) && keys_equal(key, &candidate) {
            matches.push(record);
        }
    }
    Ok(matches)
}

/// Compara dos claves de igualdad (`NULL` nunca iguala; `Int↔Float` con coerción).
///
/// Args:
///     left: Clave exterior.
///     right: Clave interior.
///
/// Returns:
///     `true` si ambas claves no nulas son iguales.
fn keys_equal(left: &ScalarValue, right: &ScalarValue) -> bool {
    match (left, right) {
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => false,
        (ScalarValue::Int(first), ScalarValue::Int(second)) => first == second,
        (ScalarValue::UInt(first), ScalarValue::UInt(second)) => first == second,
        (ScalarValue::Text(first), ScalarValue::Text(second)) => first == second,
        (ScalarValue::Bool(first), ScalarValue::Bool(second)) => first == second,
        (ScalarValue::Float(first), ScalarValue::Float(second)) => {
            first.partial_cmp(second).is_some_and(|order| order.is_eq())
        }
        (ScalarValue::Int(number), ScalarValue::Float(other))
        | (ScalarValue::Float(other), ScalarValue::Int(number)) => (*number as f64)
            .partial_cmp(other)
            .is_some_and(|order| order.is_eq()),
        (ScalarValue::UInt(number), ScalarValue::Float(other))
        | (ScalarValue::Float(other), ScalarValue::UInt(number)) => (*number as f64)
            .partial_cmp(other)
            .is_some_and(|order| order.is_eq()),
        (ScalarValue::Int(first), ScalarValue::UInt(second)) => {
            (*first >= 0) && (*first as u64 == *second)
        }
        (ScalarValue::UInt(first), ScalarValue::Int(second)) => {
            (*second >= 0) && (*first == *second as u64)
        }
        _ => false,
    }
}

/// Fusiona un par emparejado con columnas prefijadas `tabla.columna`.
///
/// Args:
///     sides: Lados resueltos (aportan los nombres de tabla).
///     outer: Registro exterior.
///     inner: Registro interior.
///
/// Returns:
///     La fila fusionada (ausente → `NULL`).
fn merge_rows(sides: &JoinSides, outer: &Record, inner: &Record) -> Row {
    let mut row = Row::new();
    for column in sides
        .outer
        .columns
        .iter()
        .map(|definition| &definition.name)
    {
        let value = outer
            .scalars
            .get(column)
            .cloned()
            .unwrap_or(ScalarValue::Null);
        row.insert(format!("{}.{column}", sides.outer.name), value);
    }
    for column in sides
        .inner
        .columns
        .iter()
        .map(|definition| &definition.name)
    {
        let value = inner
            .scalars
            .get(column)
            .cloned()
            .unwrap_or(ScalarValue::Null);
        row.insert(format!("{}.{column}", sides.inner.name), value);
    }
    row
}

/// Proyecta todas las filas fusionadas y aplica `LIMIT`.
///
/// Args:
///     sides: Lados resueltos (esquemas para validar la proyección).
///     merged: Filas fusionadas con columnas prefijadas.
///     select: Consulta (aporta proyección y `LIMIT`).
///
/// Returns:
///     Las filas de salida (hasta `LIMIT`).
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna no existe;
///     [`RuscaError::TypeMismatch`] si una columna no cualificada colisiona.
fn project_all(sides: &JoinSides, merged: &[Row], select: &Select) -> Result<Vec<Row>, RuscaError> {
    let limit = select.limit.map_or(usize::MAX, |value| value as usize);
    let mut rows = Vec::new();
    for fused in merged {
        if rows.len() >= limit {
            break;
        }
        rows.push(project_row(sides, fused, &select.projection)?);
    }
    Ok(rows)
}

/// Proyecta una fila fusionada (cualificadas directas; resto con regla de colisión).
///
/// Args:
///     sides: Lados resueltos (esquemas para validar la proyección).
///     fused: Fila fusionada con columnas prefijadas.
///     projection: Proyección pedida (`*` o lista de columnas).
///
/// Returns:
///     La fila de salida.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si la columna (o su tabla) no existe;
///     [`RuscaError::TypeMismatch`] si una columna sin cualificar está en ambas.
fn project_row(sides: &JoinSides, fused: &Row, projection: &Projection) -> Result<Row, RuscaError> {
    let Projection::Columns(columns) = projection else {
        return Ok(fused.clone());
    };
    let mut row = Row::new();
    for name in columns {
        if let Some((table, column)) = name.split_once('.') {
            row.insert(name.clone(), qualified_value(sides, fused, table, column)?);
        } else {
            let (owner, value) = unqualified_value(sides, fused, name)?;
            row.insert(owner, value);
        }
    }
    Ok(row)
}

/// Resuelve una columna cualificada `tabla.columna` en la fila fusionada.
///
/// Args:
///     sides: Lados resueltos (para validar tabla y columna).
///     fused: Fila fusionada con columnas prefijadas.
///     table: Tabla pedida (debe ser una de las dos).
///     column: Columna pedida (debe existir en su tabla).
///
/// Returns:
///     El valor fusionado (o `NULL` si falta).
///
/// Errors:
///     [`RuscaError::TableNotFound`] si la tabla no participa en el `JOIN`;
///     [`RuscaError::ColumnNotFound`] si la columna no existe en su tabla.
fn qualified_value(
    sides: &JoinSides,
    fused: &Row,
    table: &str,
    column: &str,
) -> Result<ScalarValue, RuscaError> {
    let definition = if table == sides.outer.name {
        &sides.outer
    } else if table == sides.inner.name {
        &sides.inner
    } else {
        return Err(RuscaError::TableNotFound {
            table: table.to_string(),
        });
    };
    definition.column_type(column)?;
    Ok(fused
        .get(&format!("{table}.{column}"))
        .cloned()
        .unwrap_or(ScalarValue::Null))
}

/// Resuelve una columna sin cualificar (única dueña o error por colisión).
///
/// Args:
///     sides: Lados resueltos (esquemas para localizar la columna).
///     fused: Fila fusionada con columnas prefijadas.
///     name: Columna pedida sin cualificar.
///
/// Returns:
///     `(clave de salida, valor)`: la clave es `name` tal cual.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si no está en ninguna tabla;
///     [`RuscaError::TypeMismatch`] si está en ambas (exige cualificación).
fn unqualified_value(
    sides: &JoinSides,
    fused: &Row,
    name: &str,
) -> Result<(String, ScalarValue), RuscaError> {
    let in_outer = sides.outer.column_type(name).is_ok();
    let in_inner = sides.inner.column_type(name).is_ok();
    if in_outer && in_inner {
        return Err(RuscaError::TypeMismatch {
            message: format!(
                "la columna '{name}' está en ambas tablas (cualifícala como '{}.{name}' o '{}.{name}')",
                sides.outer.name, sides.inner.name
            ),
        });
    }
    let Some(owner) = (if in_outer {
        Some(sides.outer.name.clone())
    } else if in_inner {
        Some(sides.inner.name.clone())
    } else {
        None
    }) else {
        return Err(RuscaError::ColumnNotFound {
            column: name.to_string(),
        });
    };
    Ok((
        name.to_string(),
        fused
            .get(&format!("{owner}.{name}"))
            .cloned()
            .unwrap_or(ScalarValue::Null),
    ))
}
