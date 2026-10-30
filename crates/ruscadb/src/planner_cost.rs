//! Planificador basado en costos con estimacion aprendida (SPEC-0142/0174).
//!
//! El estimador clasico usa formulas cerradas (filas x factor). Aqui se anade
//! una **estimacion de selectividad por indice aprendido**: dado un conjunto
//! ordenado de claves, [`LearnedIndex`] localiza los extremos del rango en
//! `O(log epsilon)` y devuelve cuantas claves distintas caen dentro. Es el uso
//! clasico de los modelos aprendidos en optimizacion de consultas (Kraska et
//! al., SIGMOD 2018).

use ruscadb_core::{RuscaError, ScalarValue};

use crate::catalog::Catalog;
use crate::database::Database;
use crate::executor::is_visible;
use crate::heap::heap_scan;
#[cfg(feature = "learned-index")]
use crate::learned_index::LearnedIndex;

/// Estimador de costos del planificador.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CostEstimator;

impl CostEstimator {
    /// Coste de un escaneo secuencial.
    ///
    /// Args:
    ///     rows: Filas estimadas.
    ///     factor: Coste por fila.
    ///
    /// Returns:
    ///     Coste total.
    pub fn estimate_scan(rows: usize, factor: f64) -> f64 {
        rows as f64 * factor
    }

    /// Coste de un filtro.
    ///
    /// Args:
    ///     rows: Filas de entrada.
    ///     selectivity: Fraccion que sobrevive.
    ///     factor: Coste por fila.
    ///
    /// Returns:
    ///     Coste total.
    pub fn estimate_filter(rows: usize, selectivity: f64, factor: f64) -> f64 {
        (rows as f64 * selectivity) * factor
    }

    /// Coste de un join nested-loop.
    ///
    /// Args:
    ///     left_rows: Filas de la entrada izquierda.
    ///     right_rows: Filas de la entrada derecha.
    ///     factor: Coste por par.
    ///
    /// Returns:
    ///     Coste total.
    pub fn estimate_join(left_rows: usize, right_rows: usize, factor: f64) -> f64 {
        (left_rows as f64 * right_rows as f64) * factor
    }

    /// Coste de una ordenacion.
    ///
    /// Args:
    ///     rows: Filas a ordenar.
    ///     factor: Coste por comparacion.
    ///
    /// Returns:
    ///     Coste total (`n log n`).
    pub fn estimate_sort(rows: usize, factor: f64) -> f64 {
        (rows as f64 * (rows as f64).log2()) * factor
    }

    #[cfg(feature = "learned-index")]
    /// Estima cuantas claves distintas caen en `[low, high]` con un indice
    /// aprendido sobre `keys`.
    ///
    /// Args:
    ///     keys: Claves ordenadas (se deduplican internamente).
    ///     low: Limite inferior inclusivo.
    ///     high: Limite superior inclusivo.
    ///
    /// Returns:
    ///     Numero de claves distintas en el rango.
    pub fn learned_range(keys: &[i64], low: i64, high: i64) -> usize {
        if high < low {
            return 0;
        }
        let index = LearnedIndex::build(keys, 8);
        let upper = index.rank_lower_bound(high.saturating_add(1));
        let lower = index.rank_lower_bound(low);
        upper.saturating_sub(lower)
    }
}

#[cfg(feature = "learned-index")]
impl Database {
    /// Estima las claves distintas de `table.column` en `[low, high]`.
    ///
    /// Extrae los valores `Int` visibles (MVCC), los ordena y usa
    /// [`CostEstimator::learned_range`]. Pensado para selectivity estimation
    /// sobre columnas con indice ordenado.
    ///
    /// Args:
    ///     table: Tabla a recorrer.
    ///     column: Columna `Int`.
    ///     low: Limite inferior inclusivo.
    ///     high: Limite superior inclusivo.
    ///
    /// Returns:
    ///     Numero estimado de claves distintas en el rango.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe; errores de heap.
    pub fn estimate_distinct_range(
        &mut self,
        table: &str,
        column: &str,
        low: i64,
        high: i64,
    ) -> Result<usize, RuscaError> {
        if high < low {
            return Ok(0);
        }
        let catalog = Catalog::load(self)?;
        let definition = catalog.get(table)?.clone();
        let snapshot = self.snapshot();
        let mut keys: Vec<i64> = Vec::new();
        for (_, record) in heap_scan(self, &definition)? {
            if !is_visible(&snapshot, &record) {
                continue;
            }
            if let Some(ScalarValue::Int(value)) = record.scalars.get(column) {
                keys.push(*value);
            }
        }
        keys.sort_unstable();
        keys.dedup();
        Ok(CostEstimator::learned_range(&keys, low, high))
    }
}

#[cfg(test)]
mod tests {
    use super::CostEstimator;

    /// AC-0142-01: el coste crece con las filas.
    #[test]
    // @spec AC-0142-01
    fn test_ac_0142_01_cost_estimate() {
        let small = CostEstimator::estimate_scan(100, 1.0);
        let large = CostEstimator::estimate_scan(10_000, 1.0);
        assert!(large > small);
    }

    #[cfg(feature = "learned-index")]
    /// La estimacion aprendida coincide con el conteo exacto del rango.
    #[test]
    fn learned_range_matches_exact_count() {
        let keys: Vec<i64> = (0..200).map(|index| index * 3).collect();
        for (low, high, expected) in [
            (0i64, 0i64, 1usize),
            (3, 9, 3),
            (10, 11, 0),
            (-10, 1000, 200),
            (1000, -10, 0),
        ] {
            assert_eq!(
                CostEstimator::learned_range(&keys, low, high),
                expected,
                "rango [{low}, {high}]"
            );
        }
        assert_eq!(CostEstimator::learned_range(&[], 0, 10), 0);
    }

    /// El coste de filtro y join respeta la selectividad.
    #[test]
    fn cost_formulas() {
        assert_eq!(CostEstimator::estimate_filter(100, 0.1, 2.0), 20.0);
        assert_eq!(CostEstimator::estimate_join(2, 3, 1.5), 9.0);
        assert!(CostEstimator::estimate_sort(1, 1.0) >= 0.0);
    }
}
