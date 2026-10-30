//! Cache de resultados de consultas (SPEC-0159): TTL, capacidad y metricas.
//!
//! Cache determinista sobre `BTreeMap`: clave normalizada -> resultado.
//! Soporta TTL opcional (`get_at`), capacidad maxima con eviccion por menos
//! usados y metricas (`hits`/`misses`/`evictions`/`expired`).
//!
//! Complejidad: `get`/`set` son `O(log n)`; la eviccion recorre `n` solo al
//! insertar con el cache lleno (nunca `O(n^2)` en el camino caliente).

use std::collections::BTreeMap;

/// Entrada del cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    /// Resultado cacheado.
    pub value: String,
    /// Instante de insercion (ms).
    pub inserted_ms: u64,
    /// Numero de aciertos de esta entrada.
    pub hits: u64,
}

/// Metricas del cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Aciertos.
    pub hits: u64,
    /// Fallos.
    pub misses: u64,
    /// Entradas desalojadas por capacidad.
    pub evictions: u64,
    /// Entradas expiradas por TTL.
    pub expired: u64,
}

impl CacheStats {
    /// Hit rate en `0..=1` (0 si no hubo consultas).
    ///
    /// Returns:
    ///     `hits / (hits + misses)`.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Cache de resultados.
#[derive(Debug, Clone)]
pub struct QueryCache {
    cache: BTreeMap<String, CacheEntry>,
    capacity: usize,
    ttl_ms: Option<u64>,
    stats: CacheStats,
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryCache {
    /// Crea el cache con capacidad 1024 y sin TTL.
    ///
    /// Returns:
    ///     Cache vacio.
    pub fn new() -> Self {
        Self {
            cache: BTreeMap::new(),
            capacity: 1024,
            ttl_ms: None,
            stats: CacheStats::default(),
        }
    }

    /// Crea el cache con limites explicitos.
    ///
    /// Args:
    ///     capacity: Entradas maximas (minimo 1).
    ///     ttl_ms: TTL en ms (`None` = sin expiracion).
    ///
    /// Returns:
    ///     Cache configurado.
    pub fn with_limits(capacity: usize, ttl_ms: Option<u64>) -> Self {
        Self {
            cache: BTreeMap::new(),
            capacity: capacity.max(1),
            ttl_ms,
            stats: CacheStats::default(),
        }
    }

    /// Inserta un resultado sin reloj (compatibilidad).
    ///
    /// Args:
    ///     key: Clave normalizada.
    ///     value: Resultado.
    pub fn set(&mut self, key: String, value: String) {
        self.set_at(key, value, 0);
    }

    /// Inserta un resultado con marca de tiempo.
    ///
    /// Args:
    ///     key: Clave normalizada.
    ///     value: Resultado.
    ///     now_ms: Instante actual (ms).
    pub fn set_at(&mut self, key: String, value: String, now_ms: u64) {
        if self.cache.len() >= self.capacity && !self.cache.contains_key(&key) {
            self.evict_one();
        }
        self.cache.insert(
            key,
            CacheEntry {
                value,
                inserted_ms: now_ms,
                hits: 0,
            },
        );
    }

    /// Lee un resultado sin comprobar TTL (compatibilidad).
    ///
    /// Args:
    ///     key: Clave.
    ///
    /// Returns:
    ///     Referencia al resultado, si existe.
    pub fn get(&self, key: &str) -> Option<&String> {
        self.cache.get(key).map(|entry| &entry.value)
    }

    /// Lee un resultado comprobando TTL y actualizando metricas.
    ///
    /// Args:
    ///     key: Clave.
    ///     now_ms: Instante actual (ms).
    ///
    /// Returns:
    ///     Referencia al resultado vigente, o `None` si falta/expiro.
    pub fn get_at(&mut self, key: &str, now_ms: u64) -> Option<&String> {
        let expired = self
            .ttl_ms
            .zip(self.cache.get(key).map(|entry| entry.inserted_ms))
            .is_some_and(|(ttl, inserted)| now_ms.saturating_sub(inserted) > ttl);
        if expired {
            self.cache.remove(key);
            self.stats.expired += 1;
            self.stats.misses += 1;
            return None;
        }
        match self.cache.get_mut(key) {
            Some(entry) => {
                entry.hits += 1;
                self.stats.hits += 1;
                Some(&entry.value)
            }
            None => {
                self.stats.misses += 1;
                None
            }
        }
    }

    /// Numero de entradas.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Indica si el cache esta vacio.
    ///
    /// Returns:
    ///     `true` sin entradas.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Vacia el cache (no reinicia metricas).
    pub fn invalidate(&mut self) {
        self.cache.clear();
    }

    /// Elimina las entradas expiradas.
    ///
    /// Args:
    ///     now_ms: Instante actual (ms).
    ///
    /// Returns:
    ///     Numero de entradas expiradas.
    pub fn evict_expired(&mut self, now_ms: u64) -> usize {
        let Some(ttl) = self.ttl_ms else {
            return 0;
        };
        let before = self.cache.len();
        self.cache
            .retain(|_, entry| now_ms.saturating_sub(entry.inserted_ms) <= ttl);
        let expired = before - self.cache.len();
        self.stats.expired += expired as u64;
        expired
    }

    /// Metricas acumuladas.
    ///
    /// Returns:
    ///     Copia de las metricas.
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Desaloja la entrada con menos aciertos (desempate: mas antigua, clave).
    fn evict_one(&mut self) {
        let victim = self
            .cache
            .iter()
            .min_by(|(left_key, left), (right_key, right)| {
                left.hits
                    .cmp(&right.hits)
                    .then_with(|| left.inserted_ms.cmp(&right.inserted_ms))
                    .then_with(|| left_key.cmp(right_key))
            })
            .map(|(key, _)| key.clone());
        if let Some(key) = victim {
            self.cache.remove(&key);
            self.stats.evictions += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QueryCache;

    #[test] // @spec AC-0159-01
    fn test_ac_0159_01_query_cache() {
        let mut cache = QueryCache::new();
        cache.set("q1".to_string(), "result1".to_string());
        assert_eq!(cache.get("q1"), Some(&"result1".to_string()));
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
    }

    /// Capacidad: al llenarse desaloja la entrada con menos aciertos.
    #[test]
    fn capacity_evicts_least_used() {
        let mut cache = QueryCache::with_limits(2, None);
        cache.set_at("hot".to_string(), "h".to_string(), 0);
        cache.set_at("cold".to_string(), "c".to_string(), 0);
        assert_eq!(cache.get_at("hot", 0), Some(&"h".to_string()));
        cache.set_at("new".to_string(), "n".to_string(), 1);
        assert_eq!(cache.len(), 2);
        assert!(cache.get("cold").is_none(), "se desaloja la menos usada");
        assert!(cache.get("hot").is_some());
        assert_eq!(cache.stats().evictions, 1);
    }

    /// TTL: una entrada expirada no se sirve y cuenta como miss/expired.
    #[test]
    fn ttl_expires_entries() {
        let mut cache = QueryCache::with_limits(4, Some(100));
        cache.set_at("q".to_string(), "r".to_string(), 1_000);
        assert_eq!(cache.get_at("q", 1_050), Some(&"r".to_string()));
        assert_eq!(cache.get_at("q", 1_200), None);
        assert_eq!(cache.stats().expired, 1);
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hits, 1);
        assert!(cache.stats().hit_rate() > 0.0 && cache.stats().hit_rate() < 1.0);

        cache.set_at("a".to_string(), "1".to_string(), 0);
        cache.set_at("b".to_string(), "2".to_string(), 1_000);
        // Ambos superan el TTL de 100 ms en now=1200.
        assert_eq!(cache.evict_expired(1_200), 2);
        cache.invalidate();
        assert!(cache.is_empty());
    }
}
