//! Caché semántico de KNN + coste por query (SPEC-0065).
//!
//! El caché reutiliza respuestas de consultas KNN semánticamente iguales:
//! clave = embedding de consulta, hit si `cos(consulta, clave) >= 0.8`. Con el
//! modo **OFF** (por defecto) las claves se sondean y almacenan sin alterar
//! resultados ni costar nada en hits/misses; con **ON** se sirven hits y se
//! contabilizan métricas. El coste por query acumula consultas, filas y tiempo
//! de la fachada (independiente del caché).

use crate::database::Database;
use crate::executor::Row;
use ruscadb_core::RuscaError;
use std::path::{Path, PathBuf};

/// Umbral de coseno para el hit semántico (Gupta et al. 2024 ~0.8).
const SEMANTIC_CACHE_THRESHOLD: f64 = 0.8;

/// Versión del formato del caché persistido en `<data>.cache.json` (SPEC-0093).
const CACHE_FORMAT_VERSION: u64 = 1;

/// Métricas del caché semántico (SPEC-0065).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheMetrics {
    /// Consultas servidas desde el caché (solo con el modo habilitado).
    pub hits: u64,
    /// Consultas no servidas (solo con el modo habilitado).
    pub misses: u64,
    /// Entradas almacenadas (sondean también en OFF).
    pub entries: usize,
}

/// Reporte de coste por query (SPEC-0065).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostReport {
    /// Consultas ejecutadas por la fachada.
    pub queries: u64,
    /// Suma de filas devueltas (incluidas las servidas por caché).
    pub rows_returned: u64,
    /// Nanosegundos acumulados en ejecución.
    pub nanos: u64,
}

/// Entrada del caché: firma de query, clave vectorial y resultado servido.
pub(crate) struct CacheEntry {
    /// Firma de la query (texto con el vector enmascarado): tabla, k, WHERE...
    signature: String,
    /// Vector de consulta (clave de similitud).
    vector: Vec<f32>,
    /// Filas devueltas para esa clave.
    rows: Vec<Row>,
}

/// Estado interno del caché semántico (SPEC-0065).
#[derive(Default)]
pub(crate) struct SemanticCache {
    /// Modo habilitado (OFF por defecto).
    pub(crate) enabled: bool,
    /// Entradas almacenadas.
    entries: Vec<CacheEntry>,
    /// Hits contabilizados con ON.
    hits: u64,
    /// Misses contabilizados con ON.
    misses: u64,
}

impl Database {
    /// Habilita o deshabilita el servicio de hits del caché semántico.
    ///
    /// En OFF las claves siguen sondearse/almacenarse (para calentar el caché)
    /// pero no se sirven hits ni se contabilizan métricas.
    ///
    /// Args:
    ///     enabled: `true` activa hits y métricas.
    pub fn set_semantic_cache(&mut self, enabled: bool) {
        self.semantic_cache.enabled = enabled;
    }
    /// Métricas acumuladas del caché semántico.
    ///
    /// Returns:
    ///     Hits/misses (solo contados con ON) y número de entradas.
    pub fn semantic_cache_metrics(&self) -> CacheMetrics {
        CacheMetrics {
            hits: self.semantic_cache.hits,
            misses: self.semantic_cache.misses,
            entries: self.semantic_cache.entries.len(),
        }
    }

    /// Vacía el caché y resetea sus métricas (conserva el modo habilitado).
    ///
    /// También retira el sidecar `<data>.cache.json` (SPEC-0093) para que una
    /// limpieza explícita no quede revivida por una restauración posterior.
    pub fn clear_semantic_cache(&mut self) {
        self.semantic_cache.entries.clear();
        self.semantic_cache.hits = 0;
        self.semantic_cache.misses = 0;
        remove_cache_file(&self.cache_path);
    }

    /// Persiste las entradas del caché semántico en `<data>.cache.json`.
    ///
    /// Formato versionado (`version: 1`) con firma, vector clave y filas de cada
    /// entrada; la escritura es atómica (`tmp` + `rename`). Sin entradas retira
    /// el sidecar previo. En bases **cifradas** no se persiste nada en claro
    /// (el `cache.json` se retira): los datos del caché son filas reales.
    ///
    /// Args: ninguno (usa el estado en memoria y la ruta del sidecar
    /// `<data>.cache.json`).
    ///
    /// Returns:
    ///     `Ok(())` tras persistir o retirar el sidecar.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura o el `rename`;
    ///     [`RuscaError::InvalidConfig`] si una entrada no fuese serializable.
    pub fn persist_semantic_cache(&self) -> Result<(), RuscaError> {
        if self.encryption_key().is_some() || self.semantic_cache.entries.is_empty() {
            remove_cache_file(&self.cache_path);
            return Ok(());
        }
        let mut entries = Vec::with_capacity(self.semantic_cache.entries.len());
        for entry in &self.semantic_cache.entries {
            entries.push(serde_json::json!({
                "signature": entry.signature,
                "vector": entry.vector,
                "rows": entry.rows,
            }));
        }
        let payload = serde_json::json!({
            "version": CACHE_FORMAT_VERSION,
            "entries": entries,
        });
        let text = serde_json::to_string(&payload).map_err(|error| {
            RuscaError::InvalidConfig(format!("caché semántico no serializable: {error}"))
        })?;
        write_cache_file(&self.cache_path, &text)
    }

    /// Restaura el caché semántico desde `<data>.cache.json` (best-effort).
    ///
    /// Un sidecar ausente, ilegible, con versión desconocida o malformado se
    /// ignora (caché vacío), nunca impide abrir la base. En bases cifradas no se
    /// carga (no se confía en un sidecar en claro).
    pub(crate) fn restore_semantic_cache(&mut self) {
        self.semantic_cache.entries = if self.encryption_key().is_some() {
            Vec::new()
        } else {
            read_cache_file(&self.cache_path).unwrap_or_default()
        };
    }

    /// Reporte de coste por query acumulado.
    ///
    /// Returns:
    ///     Copia del [`CostReport`] vigente.
    pub fn cost_report(&self) -> CostReport {
        self.cost_report
    }

    /// Pone a cero el reporte de coste.
    pub fn reset_cost_report(&mut self) {
        self.cost_report = CostReport::default();
    }

    /// Busca un hit semántico para la firma y el vector de consulta (solo con ON).
    ///
    /// Args:
    ///     signature: Firma de la query (tabla, k, WHERE... con el vector `[?]`).
    ///     vector: Vector de consulta parseado del RQL.
    ///
    /// Returns:
    ///     Las filas cacheadas si firma idéntica y `cos >= umbral` (hit), o
    ///     `None` si no (miss). Contabiliza la métrica correspondiente.
    pub(crate) fn cache_lookup(&mut self, signature: &str, vector: &[f32]) -> Option<Vec<Row>> {
        let hit = self.semantic_cache.entries.iter().find(|entry| {
            entry.signature == signature
                && cosine(vector, &entry.vector) >= SEMANTIC_CACHE_THRESHOLD
        });
        match hit {
            Some(entry) => {
                self.semantic_cache.hits += 1;
                Some(entry.rows.clone())
            }
            None => {
                self.semantic_cache.misses += 1;
                None
            }
        }
    }

    /// Almacena (o actualiza) la entrada de la MISMA firma y clave similar.
    ///
    /// Se invoca con o sin el modo habilitado: permite calentar el caché y
    /// garantizar hits futuros (AC-0065-01/05). Una firma distinta nunca
    /// reemplaza a otra entrada (AC-0069-02/03).
    ///
    /// Args:
    ///     signature: Firma de la query.
    ///     vector: Vector de consulta (clave).
    ///     rows: Filas servidas por el motor.
    pub(crate) fn cache_store(&mut self, signature: String, vector: Vec<f32>, rows: Vec<Row>) {
        if let Some(entry) = self.semantic_cache.entries.iter_mut().find(|entry| {
            entry.signature == signature
                && cosine(&vector, &entry.vector) >= SEMANTIC_CACHE_THRESHOLD
        }) {
            entry.rows = rows;
            return;
        }
        self.semantic_cache.entries.push(CacheEntry {
            signature,
            vector,
            rows,
        });
    }

    /// Invalida las ENTRADAS del caché tras una mutación (SPEC-0069).
    ///
    /// Conserva los contadores acumulados de hits/misses: la invalidación es
    /// de datos, no de telemetría. También retira el sidecar persistido
    /// `<data>.cache.json` (SPEC-0093) para que un crash antes de re-persistir
    /// no resucite filas obsoletas en la próxima apertura.
    pub(crate) fn invalidate_semantic_cache(&mut self) {
        // Cualquier escritura invalida tambien el cache exacto de resultados.
        self.query_cache.invalidate();
        if self.semantic_cache.entries.is_empty() {
            return;
        }
        self.semantic_cache.entries.clear();
        remove_cache_file(&self.cache_path);
    }

    /// Acumula una consulta ejecutada en el reporte de coste.
    ///
    /// Args:
    ///     rows_returned: Filas devueltas (o servidas por caché).
    ///     nanos: Nanosegundos de la ejecución.
    pub(crate) fn note_execution(&mut self, rows_returned: usize, nanos: u64) {
        self.cost_report.queries += 1;
        self.cost_report.rows_returned += rows_returned as u64;
        self.cost_report.nanos += nanos;
    }
}

/// Lee y valida el sidecar del caché semántico `<data>.cache.json`.
///
/// Args:
///     path: Ruta del sidecar.
///
/// Returns:
///     `Some(entradas)` si el fichero existe, es JSON válido, declara la
///     versión soportada y todas las entradas son bien formadas; `None` en
///     cualquier otro caso (se ignora, sin error).
fn read_cache_file(path: &Path) -> Option<Vec<CacheEntry>> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(CACHE_FORMAT_VERSION) {
        return None;
    }
    let items = value.get("entries")?.as_array()?;
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let signature = item.get("signature")?.as_str()?.to_string();
        let vector: Vec<f32> = serde_json::from_value(item.get("vector")?.clone()).ok()?;
        let rows: Vec<Row> = serde_json::from_value(item.get("rows")?.clone()).ok()?;
        entries.push(CacheEntry {
            signature,
            vector,
            rows,
        });
    }
    Some(entries)
}

/// Escribe el sidecar del caché de forma atómica (`tmp` + `rename`).
///
/// Args:
///     path: Ruta destino del sidecar.
///     text: Contenido JSON a escribir.
///
/// Returns:
///     `Ok(())` si el fichero quedó publicado atómicamente.
///
/// Errors:
///     [`RuscaError::Io`] si falla la escritura o el `rename`.
fn write_cache_file(path: &Path, text: &str) -> Result<(), RuscaError> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Retira el sidecar del caché si existe (best-effort, sin error).
///
/// Args:
///     path: Ruta del sidecar a retirar.
fn remove_cache_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Similitud de coseno entre dos vectores.
///
/// Args:
///     a: Primer vector.
///     b: Segundo vector.
///
/// Returns:
///     El coseno en `[-1.0, 1.0]` (0 si alguno es el vector nulo).
fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum();
    let norm_a: f64 = a
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();
    let norm_b: f64 = b
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Extrae el vector de consulta de una sentencia KNN, si la hay.
///
/// Localiza `KNN` y toma el primer `[...]` posterior; si no parsea números
/// devuelve `None` (la query no es cacheable).
///
/// Args:
///     query: Texto RQL.
///
/// Returns:
///     El vector de consulta como `f32`, o `None` si no es KNN cacheable.
pub(crate) fn extract_knn_vector(query: &str) -> Option<Vec<f32>> {
    let marker = query.rfind("KNN")?;
    let rest = &query[marker + "KNN".len()..];
    let start = rest.find('[')?;
    let tail = &rest[start + 1..];
    let end = tail.find(']')?;
    let body = &tail[..end];
    let values: Vec<f32> = body
        .split(',')
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

/// Firma de una query KNN: texto normalizado con el vector enmascarado.
///
/// Dos consultas con la misma firma difieren solo en los valores del vector
/// (tabla, `k`, `WHERE` y `LIMIT` forman parte de la firma, SPEC-0069).
///
/// Args:
///     query: Texto RQL.
///
/// Returns:
///     La firma (p. ej. `SELECT * FROM items KNN embedding <|2|> [?]`), o
///     `None` si la query no es un KNN cacheable.
pub(crate) fn query_signature(query: &str) -> Option<String> {
    let marker = query.rfind("KNN")?;
    let rest = &query[marker..];
    let start = rest.find('[')?;
    let tail = &rest[start + 1..];
    let end = tail.find(']')?;
    let mut masked = String::with_capacity(query.len());
    masked.push_str(&query[..marker]);
    masked.push_str(&rest[..start]);
    masked.push_str("[?]");
    masked.push_str(&tail[end + 1..]);
    Some(masked.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// Un KNN con vector se extrae correctamente.
    #[test]
    fn extracts_knn_vector() {
        let query = "SELECT * FROM items KNN embedding <|2|> [0.1, 0.2, 0.3]";
        assert_eq!(extract_knn_vector(query), Some(vec![0.1, 0.2, 0.3]));
    }

    /// Una query sin KNN no es cacheable.
    #[test]
    fn non_knn_is_not_cacheable() {
        assert_eq!(extract_knn_vector("SELECT * FROM items"), None);
        assert_eq!(
            extract_knn_vector("SELECT * FROM t WHERE MATCH(a, '[x]')"),
            None
        );
    }

    /// El coseno de vectores idénticos es 1 y de ortogonales es 0.
    #[test]
    fn cosine_bounds() {
        let a = [1.0_f32, 0.0];
        let b = [0.0_f32, 1.0];
        assert_eq!(cosine(&a, &a), 1.0);
        assert!(cosine(&a, &b).abs() < 1e-9);
    }

    /// Un error de parseo en los valores devuelve `None` (no cacheable).
    #[test]
    fn malformed_knn_vector_is_not_cacheable() {
        assert_eq!(extract_knn_vector("SELECT * FROM t KNN v <|1|> [xx]"), None);
    }
}
