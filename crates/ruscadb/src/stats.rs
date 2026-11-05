//! Estadísticas de tabla para el planner por coste (SPEC-0201).
//!
//! `ANALYZE` (API `Database::analyze`) recorre las filas visibles en una pasada
//! `O(N)` y calcula filas, nulos, distintos, min/max y un histograma equi-ancho
//! para columnas numéricas. El sidecar `<data>.stats.json` se escribe de forma
//! atómica y el planner lo usa solo si la tabla no ha mutado desde el análisis.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use ruscadb_core::ScalarValue;
use serde::{Deserialize, Serialize};

use crate::catalog::{ColumnType, TableDef};

/// Número de buckets del histograma equi-ancho.
pub const HISTOGRAM_BUCKETS: usize = 32;

/// Estadísticas de una columna.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    /// Valores `NULL` (incluye columnas ausentes).
    pub nulls: u64,
    /// Valores distintos no nulos (exacto).
    pub distinct: u64,
    /// Mínimo no nulo.
    pub min: Option<ScalarValue>,
    /// Máximo no nulo.
    pub max: Option<ScalarValue>,
    /// Conteos por bucket (vacío si la columna no es numérica).
    pub histogram: Vec<u64>,
}

/// Estadísticas de una tabla.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableStats {
    /// Filas visibles en el momento del análisis.
    pub rows: u64,
    /// Estadísticas por columna.
    pub columns: BTreeMap<String, ColumnStats>,
}

/// Contenido del sidecar de estadísticas.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsFile {
    /// Época de mutaciones al analizar.
    pub epoch: u64,
    /// Estadísticas por tabla.
    pub tables: BTreeMap<String, TableStats>,
}

impl TableStats {
    /// Construye las estadísticas a partir de filas visibles.
    pub fn from_records(definition: &TableDef, records: &[ruscadb_core::Record]) -> Self {
        let mut columns = BTreeMap::new();
        for column in &definition.columns {
            let mut nulls = 0_u64;
            let mut distinct = BTreeSet::new();
            let mut min: Option<ScalarValue> = None;
            let mut max: Option<ScalarValue> = None;
            let mut numeric = Vec::new();
            for record in records {
                let value = record
                    .scalars
                    .get(&column.name)
                    .cloned()
                    .unwrap_or(ScalarValue::Null);
                if matches!(value, ScalarValue::Null) {
                    nulls += 1;
                    continue;
                }
                distinct.insert(canonical(&value));
                if let Some(current) = &min {
                    if scalar_cmp(&value, current) == Some(Ordering::Less) {
                        min = Some(value.clone());
                    }
                } else {
                    min = Some(value.clone());
                }
                if let Some(current) = &max {
                    if scalar_cmp(&value, current) == Some(Ordering::Greater) {
                        max = Some(value.clone());
                    }
                } else {
                    max = Some(value.clone());
                }
                if column.col_type != ColumnType::Text
                    && column.col_type != ColumnType::Bool
                    && let Some(number) = numeric_value(&value)
                {
                    numeric.push(number);
                }
            }
            let histogram = histogram(&numeric);
            columns.insert(
                column.name.clone(),
                ColumnStats {
                    nulls,
                    distinct: distinct.len() as u64,
                    min,
                    max,
                    histogram,
                },
            );
        }
        Self {
            rows: records.len() as u64,
            columns,
        }
    }

    /// Selectividad estimada de `column = value`.
    pub fn equality_selectivity(&self, column: &str, value: &ScalarValue) -> f64 {
        let Some(stats) = self.columns.get(column) else {
            return DEFAULT_SELECTIVITY;
        };
        if self.rows == 0 {
            return DEFAULT_SELECTIVITY;
        }
        let non_null = self.rows.saturating_sub(stats.nulls);
        if non_null == 0 {
            return 0.0;
        }
        if let (Some(number), Some(min), Some(max)) =
            (numeric_value(value), stats.min.as_ref(), stats.max.as_ref())
            && let (Some(min), Some(max)) = (numeric_value(min), numeric_value(max))
            && (number < min || number > max)
        {
            return 0.0;
        }
        let base = 1.0 / stats.distinct.max(1) as f64;
        base * (non_null as f64 / self.rows as f64)
    }

    /// Selectividad estimada de `column BETWEEN low AND high` (numérica).
    pub fn range_selectivity(&self, column: &str, low: Option<f64>, high: Option<f64>) -> f64 {
        let Some(stats) = self.columns.get(column) else {
            return DEFAULT_SELECTIVITY;
        };
        if stats.histogram.is_empty() || self.rows == 0 {
            return DEFAULT_SELECTIVITY;
        }
        let (Some(min), Some(max)) = (
            stats.min.as_ref().and_then(numeric_value),
            stats.max.as_ref().and_then(numeric_value),
        ) else {
            return DEFAULT_SELECTIVITY;
        };
        if max <= min {
            return 1.0;
        }
        let low = low.unwrap_or(min).max(min);
        let high = high.unwrap_or(max).min(max);
        if high < low {
            return 0.0;
        }
        let total: u64 = stats.histogram.iter().sum();
        if total == 0 {
            return DEFAULT_SELECTIVITY;
        }
        let width = (max - min) / stats.histogram.len() as f64;
        let mut selected = 0_u64;
        for (index, count) in stats.histogram.iter().enumerate() {
            let bucket_low = min + width * index as f64;
            let bucket_high = bucket_low + width;
            if bucket_high >= low && bucket_low <= high {
                selected += count;
            }
        }
        selected as f64 / total as f64
    }

    /// Selectividad estimada de `column IS NULL`.
    pub fn null_selectivity(&self, column: &str) -> f64 {
        let Some(stats) = self.columns.get(column) else {
            return 0.0;
        };
        if self.rows == 0 {
            return 0.0;
        }
        stats.nulls as f64 / self.rows as f64
    }

    /// Cardinalidad estimada de un join de igualdad (SPEC-0201-05).
    ///
    /// Usa `filas_izq · filas_der / max(distinct)` (independencia + containment).
    pub fn join_cardinality(&self, column: &str, other: &TableStats, other_column: &str) -> u64 {
        let (Some(left), Some(right)) = (self.columns.get(column), other.columns.get(other_column))
        else {
            return self.rows.saturating_mul(other.rows).max(1);
        };
        let non_null_left = self.rows.saturating_sub(left.nulls) as f64;
        let non_null_right = other.rows.saturating_sub(right.nulls) as f64;
        let distinct = left.distinct.max(right.distinct).max(1) as f64;
        ((non_null_left * non_null_right) / distinct).round() as u64
    }
}

/// Selectividad conservadora sin estadísticas.
pub const DEFAULT_SELECTIVITY: f64 = 0.1;

/// Serializa un escalar a una clave estable para `distinct`.
fn canonical(value: &ScalarValue) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
}

/// Extrae un valor numérico comparable.
pub(crate) fn numeric_value(value: &ScalarValue) -> Option<f64> {
    match value {
        ScalarValue::Int(number) => Some(*number as f64),
        ScalarValue::UInt(number) => Some(*number as f64),
        ScalarValue::Float(number) if number.is_finite() => Some(*number),
        ScalarValue::TimestampMillis(number) => Some(*number as f64),
        _ => None,
    }
}

/// Compara dos escalares del mismo dominio (o numéricos entre sí).
fn scalar_cmp(left: &ScalarValue, right: &ScalarValue) -> Option<Ordering> {
    match (left, right) {
        (ScalarValue::Int(first), ScalarValue::Int(second)) => Some(first.cmp(second)),
        (ScalarValue::UInt(first), ScalarValue::UInt(second)) => Some(first.cmp(second)),
        (ScalarValue::Text(first), ScalarValue::Text(second)) => Some(first.cmp(second)),
        (ScalarValue::Bool(first), ScalarValue::Bool(second)) => Some(first.cmp(second)),
        (ScalarValue::TimestampMillis(first), ScalarValue::TimestampMillis(second)) => {
            Some(first.cmp(second))
        }
        _ => numeric_value(left)
            .zip(numeric_value(right))
            .and_then(|(first, second)| first.partial_cmp(&second)),
    }
}

/// Construye un histograma equi-ancho de `HISTOGRAM_BUCKETS` buckets.
fn histogram(values: &[f64]) -> Vec<u64> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }
    if !min.is_finite() || !max.is_finite() {
        return Vec::new();
    }
    let mut buckets = vec![0_u64; HISTOGRAM_BUCKETS];
    if max == min {
        buckets[0] = values.len() as u64;
        return buckets;
    }
    let width = (max - min) / HISTOGRAM_BUCKETS as f64;
    for value in values {
        let index = (((value - min) / width).floor() as usize).min(HISTOGRAM_BUCKETS - 1);
        buckets[index] += 1;
    }
    buckets
}

use ruscadb_core::RuscaError;

use crate::catalog::Catalog;
use crate::database::Database;
use crate::executor::is_visible;
use crate::heap::heap_scan;

impl Database {
    /// Analiza una tabla y persiste `<data>.stats.json` (SPEC-0201).
    ///
    /// Args:
    ///     table: Nombre de la tabla.
    ///
    /// Returns:
    ///     Las estadísticas calculadas.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe; errores de heap
    ///     o E/S al persistir el sidecar.
    pub fn analyze(&mut self, table: &str) -> Result<TableStats, RuscaError> {
        self.ensure_writable("analyze")?;
        self.require_write("analyze")?;
        let definition = Catalog::load(self)?.get(table)?.clone();
        let snapshot = self.snapshot();
        let records: Vec<ruscadb_core::Record> = heap_scan(self, &definition)?
            .into_iter()
            .map(|(_, record)| record)
            .filter(|record| is_visible(&snapshot, record))
            .collect();
        let stats = TableStats::from_records(&definition, &records);
        self.stats.insert(table.to_string(), stats.clone());
        self.stats_epoch
            .insert(table.to_string(), self.mutation_epoch);
        // Las stats cambian el plan: invalida la caché sin invalidar las stats
        // del resto de tablas (la época de mutaciones no cambia).
        self.plan_cache.clear();
        self.plan_cache_order.clear();
        self.save_stats()?;
        Ok(stats)
    }

    /// Estadísticas vigentes de una tabla (`None` si no se ha analizado o si
    /// la tabla mutó desde el análisis).
    pub fn stats(&self, table: &str) -> Option<&TableStats> {
        if self.stats_epoch.get(table).copied() == Some(self.mutation_epoch) {
            self.stats.get(table)
        } else {
            None
        }
    }

    /// Marca una mutación para invalidar estadísticas.
    pub(crate) fn mark_mutated(&mut self) {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
    }

    /// Carga el sidecar de estadísticas (best-effort; corrupto = sin stats).
    pub(crate) fn restore_stats(&mut self) {
        let Ok(bytes) = std::fs::read(&self.stats_path) else {
            return;
        };
        let Ok(file) = serde_json::from_slice::<StatsFile>(&bytes) else {
            return;
        };
        self.stats = file.tables;
        self.stats_epoch = self
            .stats
            .keys()
            .map(|table| (table.clone(), file.epoch))
            .collect();
        self.mutation_epoch = file.epoch;
    }

    /// Escribe el sidecar de forma atómica (tmp + rename).
    pub(crate) fn save_stats(&self) -> Result<(), RuscaError> {
        let file = StatsFile {
            epoch: self.mutation_epoch,
            tables: self.stats.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|error| {
            RuscaError::Io(std::io::Error::other(format!(
                "stats: no se pudo serializar: {error}"
            )))
        })?;
        let temporary = self.stats_path.with_extension("stats.json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &self.stats_path)?;
        Ok(())
    }
}
