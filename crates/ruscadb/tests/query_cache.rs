//! Tests de aceptación de la feature `query-cache` (SPEC-0159).

#![allow(clippy::expect_used)]

use ruscadb::QueryCache;

#[test] // @spec AC-0159-01
fn test_ac_0159_01_query_cache() {
    let mut cache = QueryCache::new();
    cache.set("q1".to_string(), "result1".to_string());
    assert_eq!(cache.get("q1"), Some(&"result1".to_string()));
    assert_eq!(cache.len(), 1);
}
