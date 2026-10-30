//! Cableado time-series en la fachada (SPEC-0057, roadmap §5.2).
//!
//! Módulo NUEVO que expone `BUCKET`/`WINDOW` de alto nivel sobre
//! [`ruscadb_ts`] sin tocar el agregador existente del executor (NF-0057-01).
//! Extrae la serie de `scalars[col]` por recorrido del heap con visibilidad
//! MVCC, valida que la columna sea temporal y delega el cálculo en
//! `ruscadb_ts::{time_bucket, window}`.
//!
//! Límites (NF-0057-02): sin panics; los cubos/ventanas vacíos se omiten.

use ruscadb_core::{Record, RuscaError, ScalarValue};
use ruscadb_ts::{Agg, SeriesPoint, time_bucket, window as ts_window};

use crate::catalog::Catalog;
use crate::database::Database;
use crate::executor::is_visible;
use crate::heap::heap_scan;

/// Fila agregada de un bucket o ventana temporal (inicio inclusivo, fin
/// exclusivo, agregado en `value`).
#[derive(Clone, Debug, PartialEq)]
pub struct TsRow {
    /// Inicio del cubo/ventana en ms.
    pub start_ms: i64,
    /// Fin exclusivo del cubo/ventana en ms.
    pub end_ms: i64,
    /// Agregado calculado (nunca `None`: los huecos se omiten).
    pub value: f64,
}

impl Database {
    /// Agrupa las filas de `table` en cubos temporales de `interval_ms`.
    ///
    /// `BUCKET(ts_col, interval_ms)` alineado con `time_bucket` (piso). Los
    /// cubos vacíos se omiten (no hay NULL fantasma). `value_col` es opcional
    /// (innecesario para `Agg::Count`).
    ///
    /// Args:
    ///     table: Tabla a recorrer.
    ///     ts_col: Columna temporal (`Int`/`UInt`/`Float`/`TimestampMillis`).
    ///     interval_ms: Tamaño del cubo en ms (debe ser > 0).
    ///     value_col: Columna métrica o `None` si solo importa el conteo.
    ///     agg: Agregado por cubo (de `ruscadb_ts::Agg`).
    ///
    /// Returns:
    ///     Filas agregadas ordenadas por `start_ms`.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] / [`RuscaError::ColumnNotFound`] ante
    ///     esquema inexistente; [`RuscaError::TypeMismatch`] si la columna no
    ///     es temporal; [`RuscaError::InvalidConfig`] si el intervalo es <= 0.
    pub fn ts_bucket(
        &mut self,
        table: &str,
        ts_col: &str,
        interval_ms: i64,
        value_col: Option<&str>,
        agg: Agg,
    ) -> Result<Vec<TsRow>, RuscaError> {
        let points = extract_points(self, table, ts_col, value_col)?;
        bucket_points(&points, interval_ms, agg)
    }

    /// Ventanas deslizantes de `window_ms` con paso `step_ms` sobre `table`.
    ///
    /// Args:
    ///     table: Tabla a recorrer.
    ///     ts_col: Columna temporal.
    ///     window_ms: Tamaño de la ventana en ms (> 0).
    ///     step_ms: Paso entre ventanas (0 < step <= window).
    ///     value_col: Columna métrica o `None` (conteo).
    ///     agg: Agregado por ventana (de `ruscadb_ts::Agg`).
    ///
    /// Returns:
    ///     Ventanas con agregado (los huecos `None` se omiten).
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si `window_ms`/`step_ms` son inválidos.
    pub fn ts_window(
        &mut self,
        table: &str,
        ts_col: &str,
        window_ms: i64,
        step_ms: i64,
        value_col: Option<&str>,
        agg: Agg,
    ) -> Result<Vec<TsRow>, RuscaError> {
        let points = extract_points(self, table, ts_col, value_col)?;
        let windows = ts_window(&points, window_ms, step_ms, agg)?;
        Ok(windows
            .into_iter()
            .filter_map(|window| {
                window.value.map(|value| TsRow {
                    start_ms: window.start,
                    end_ms: window.end,
                    value,
                })
            })
            .collect())
    }

    /// Comprime la columna de valores de una serie con Gorilla (SPEC-0178).
    ///
    /// Extrae los puntos visibles de `table` (MVCC) y codifica solo la columna
    /// de valores con XOR de Gorilla. Los timestamps no se incluyen: el codec
    /// es para la columna de metricas.
    ///
    /// Args:
    ///     table: Tabla a recorrer.
    ///     ts_col: Columna temporal (valida el esquema).
    ///     value_col: Columna de valores a comprimir.
    ///
    /// Returns:
    ///     `(puntos, bytes)` con la serie codificada.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] / [`RuscaError::ColumnNotFound`] /
    ///     [`RuscaError::TypeMismatch`] si el esquema no es valido.
    pub fn ts_compress(
        &mut self,
        table: &str,
        ts_col: &str,
        value_col: &str,
    ) -> Result<(usize, Vec<u8>), RuscaError> {
        let points = extract_points(self, table, ts_col, Some(value_col))?;
        let values: Vec<f64> = points.iter().map(|point| point.value).collect();
        let codec = crate::gorilla_ts::GorillaTs::new();
        let bytes = codec.encode(&values);
        Ok((values.len(), bytes))
    }

    /// Descomprime una columna de valores codificada con [`Self::ts_compress`].
    ///
    /// Args:
    ///     bytes: Buffer Gorilla.
    ///
    /// Returns:
    ///     Valores reconstruidos bit a bit.
    pub fn ts_decompress(&self, bytes: &[u8]) -> Vec<f64> {
        crate::gorilla_ts::GorillaTs::new().decode(bytes)
    }
}

/// Calcula los cubos alineados a `interval_ms` (piso) agrupando por
/// `time_bucket` y agregando cada grupo con `ruscadb_ts::window` (DRY).
fn bucket_points(
    points: &[SeriesPoint],
    interval_ms: i64,
    agg: Agg,
) -> Result<Vec<TsRow>, RuscaError> {
    let mut groups: std::collections::BTreeMap<i64, Vec<SeriesPoint>> =
        std::collections::BTreeMap::new();
    for point in points {
        let start = time_bucket(point.ts_ms, interval_ms)?;
        groups.entry(start).or_default().push(*point);
    }
    let mut out = Vec::with_capacity(groups.len());
    for (start, group) in groups {
        // `window` con step=window devuelve exactamente un cubo con el
        // agregado; los grupos no están vacíos, así que `value` es Some.
        let windows = ts_window(&group, interval_ms, interval_ms, agg)?;
        if let Some(aggregated) = windows.into_iter().next().and_then(|w| w.value) {
            out.push(TsRow {
                start_ms: start,
                end_ms: start.saturating_add(interval_ms),
                value: aggregated,
            });
        }
    }
    Ok(out)
}

/// Extrae los puntos de la serie validando esquema y tipos (sin panics).
fn extract_points(
    database: &mut Database,
    table: &str,
    ts_col: &str,
    value_col: Option<&str>,
) -> Result<Vec<SeriesPoint>, RuscaError> {
    let catalog = Catalog::load(database)?;
    let def = catalog.get(table)?;
    validate_column(def, ts_col)?;
    if let Some(column) = value_col {
        validate_column(def, column)?;
    }
    let snapshot = database.snapshot();
    let records = heap_scan(database, def)?;
    let mut points = Vec::with_capacity(records.len());
    for (_, record) in records {
        if !is_visible(&snapshot, &record) {
            continue;
        }
        let ts_ms = temporal_ms(&record, ts_col)?;
        let value = metric_value(&record, value_col)?;
        points.push(SeriesPoint { ts_ms, value });
    }
    Ok(points)
}

/// Verifica que la columna exista en el esquema de la tabla.
fn validate_column(def: &crate::catalog::TableDef, column: &str) -> Result<(), RuscaError> {
    if def.columns.iter().any(|col| col.name == column) {
        Ok(())
    } else {
        Err(RuscaError::ColumnNotFound {
            column: column.to_string(),
        })
    }
}

/// Lee el instante de `ts_col`; cualquier no temporal es un error accionable.
///
/// Solo `Int`/`UInt`/`TimestampMillis` son temporales: `Float` y el resto
/// rechazan con `TypeMismatch` "no temporal" (AC-0057-04).
fn temporal_ms(record: &Record, ts_col: &str) -> Result<i64, RuscaError> {
    match record.scalars.get(ts_col) {
        None => Err(RuscaError::ColumnNotFound {
            column: ts_col.to_string(),
        }),
        Some(ScalarValue::Int(value)) => Ok(*value),
        Some(ScalarValue::UInt(value)) => Ok(*value as i64),
        Some(ScalarValue::TimestampMillis(value)) => Ok(*value),
        Some(other) => Err(RuscaError::TypeMismatch {
            message: format!(
                "columna '{ts_col}' no temporal: se esperaba Int/UInt/TimestampMillis, se obtuvo {other:?}"
            ),
        }),
    }
}

/// Lee la métrica numérica de `value_col` (0.0 si no se pide; `None` solo
/// ocurre cuando la fila no trae la columna pedida explícitamente).
fn metric_value(record: &Record, value_col: Option<&str>) -> Result<f64, RuscaError> {
    let Some(column) = value_col else {
        return Ok(0.0);
    };
    match record.scalars.get(column) {
        None => Err(RuscaError::ColumnNotFound {
            column: column.to_string(),
        }),
        Some(ScalarValue::Float(value)) => Ok(*value),
        Some(ScalarValue::Int(value)) => Ok(*value as f64),
        Some(ScalarValue::UInt(value)) => Ok(*value as f64),
        Some(ScalarValue::TimestampMillis(value)) => Ok(*value as f64),
        Some(other) => Err(RuscaError::TypeMismatch {
            message: format!(
                "columna '{column}' no numérica para el agregado: se obtuvo {other:?}"
            ),
        }),
    }
}
