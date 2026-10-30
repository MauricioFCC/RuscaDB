//! Cableado de vistas materializadas al executor (SPEC-0149).
//!
//! `create_materialized_view` ejecuta la consulta y congela el resultado;
//! `refresh` la vuelve a ejecutar. Las filas se guardan como texto en orden de
//! columna (claves ordenadas del `Row`), suficiente para servir dashboards y
//! exportes sin re-ejecutar la consulta en cada lectura.

use ruscadb_core::{RuscaError, ScalarValue};

use crate::database::Database;
use crate::executor::Row;
use crate::materialized_view::MaterializedView;

impl Database {
    /// Crea (o reemplaza) una vista materializada ejecutando `query`.
    ///
    /// Args:
    ///     name: Nombre de la vista.
    ///     query: Consulta RQL.
    ///
    /// Returns:
    ///     Numero de filas materializadas.
    ///
    /// Errors:
    ///     Los del executor (tabla/columna inexistente, tipo, etc.).
    pub fn create_materialized_view(
        &mut self,
        name: &str,
        query: &str,
    ) -> Result<usize, RuscaError> {
        let rows = self.execute(query)?;
        let rows = rows_to_text(&rows);
        let count = rows.len();
        self.materialized_views.insert(
            name.to_string(),
            MaterializedView {
                name: name.to_string(),
                query: query.to_string(),
                rows,
            },
        );
        Ok(count)
    }

    /// Re-ejecuta la consulta de una vista y reemplaza sus filas.
    ///
    /// Args:
    ///     name: Nombre de la vista.
    ///
    /// Returns:
    ///     Numero de filas tras el refresh.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la vista no existe; errores del
    ///     executor al re-ejecutar.
    pub fn refresh_materialized_view(&mut self, name: &str) -> Result<usize, RuscaError> {
        let query = self
            .materialized_views
            .get(name)
            .map(|view| view.query.clone())
            .ok_or_else(|| {
                RuscaError::InvalidConfig(format!("vista materializada '{name}' no existe"))
            })?;
        let rows = self.execute(&query)?;
        let rows = rows_to_text(&rows);
        let count = rows.len();
        if let Some(view) = self.materialized_views.get_mut(name) {
            view.rows = rows;
        }
        Ok(count)
    }

    /// Filas materializadas de una vista.
    ///
    /// Args:
    ///     name: Nombre de la vista.
    ///
    /// Returns:
    ///     Filas en texto, o `None` si no existe.
    pub fn materialized_view_rows(&self, name: &str) -> Option<&[Vec<String>]> {
        self.materialized_views
            .get(name)
            .map(|view| view.rows.as_slice())
    }

    /// Nombres de las vistas materializadas (orden alfabetico).
    ///
    /// Returns:
    ///     Lista de nombres.
    pub fn materialized_view_names(&self) -> Vec<String> {
        self.materialized_views.keys().cloned().collect()
    }

    /// Elimina una vista materializada.
    ///
    /// Args:
    ///     name: Nombre de la vista.
    ///
    /// Returns:
    ///     `true` si existia.
    pub fn drop_materialized_view(&mut self, name: &str) -> bool {
        self.materialized_views.remove(name).is_some()
    }
}

impl Database {
    /// Ejecuta una consulta con cache exacto de resultados (SPEC-0159).
    ///
    /// Args:
    ///     query: Consulta RQL.
    ///     now_ms: Reloj del llamador (para el TTL).
    ///
    /// Returns:
    ///     Filas del resultado (desde cache o del executor).
    ///
    /// Errors:
    ///     Los del executor, o [`RuscaError::InvalidConfig`] si el cache
    ///     contiene un JSON invalido.
    pub fn execute_cached(&mut self, query: &str, now_ms: u64) -> Result<Vec<Row>, RuscaError> {
        if let Some(cached) = self.query_cache.get_at(query, now_ms) {
            return serde_json::from_str(cached).map_err(|error| {
                RuscaError::InvalidConfig(format!("cache de resultados corrupto: {error}"))
            });
        }
        let rows = self.execute(query)?;
        let serialized = serde_json::to_string(&rows).map_err(|error| {
            RuscaError::InvalidConfig(format!("no se pudo serializar el resultado: {error}"))
        })?;
        self.query_cache
            .set_at(query.to_string(), serialized, now_ms);
        Ok(rows)
    }

    /// Configura capacidad y TTL del cache de resultados.
    ///
    /// Args:
    ///     capacity: Entradas maximas.
    ///     ttl_ms: TTL en ms (`None` = sin expiracion).
    pub fn configure_query_cache(&mut self, capacity: usize, ttl_ms: Option<u64>) {
        self.query_cache = crate::query_cache::QueryCache::with_limits(capacity, ttl_ms);
    }

    /// Vacia el cache de resultados.
    pub fn invalidate_query_cache(&mut self) {
        self.query_cache.invalidate();
    }

    /// Metricas del cache de resultados.
    ///
    /// Returns:
    ///     Copia de `CacheStats`.
    pub fn query_cache_stats(&self) -> crate::query_cache::CacheStats {
        self.query_cache.stats()
    }
}

/// Convierte filas del executor a texto (columnas ordenadas).
fn rows_to_text(rows: &[Row]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.values().map(scalar_text).collect())
        .collect()
}

/// Representacion textual estable de un escalar.
fn scalar_text(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Null => "NULL".to_string(),
        ScalarValue::Bool(flag) => flag.to_string(),
        ScalarValue::Int(number) => number.to_string(),
        ScalarValue::UInt(number) => number.to_string(),
        ScalarValue::Float(number) => number.to_string(),
        ScalarValue::Text(text) => text.clone(),
        ScalarValue::Bytes(bytes) => bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        ScalarValue::TimestampMillis(millis) => millis.to_string(),
    }
}
