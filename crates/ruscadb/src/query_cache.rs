#![allow(missing_docs)]
#![allow(dead_code)]
//! Cache de resultados (SPEC-0159): stub.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct QueryCache {
    cache: BTreeMap<String, String>,
}

impl QueryCache {
    pub fn new() -> Self {
        Self {
            cache: BTreeMap::new(),
        }
    }
    pub fn set(&mut self, key: String, value: String) {
        self.cache.insert(key, value);
    }
    pub fn get(&self, key: &str) -> Option<&String> {
        self.cache.get(key)
    }
    pub fn len(&self) -> usize {
        self.cache.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0159-01
    fn test_ac_0159_01_query_cache() {
        let mut cache = QueryCache::new();
        cache.set("q1".to_string(), "result1".to_string());
        assert_eq!(cache.get("q1"), Some(&"result1".to_string()));
        assert_eq!(cache.len(), 1);
    }
}
