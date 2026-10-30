//! Cache exacto de resultados cableado al executor (SPEC-0159).

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Abre una base temporal con `t(a INT)` y tres filas.
fn seeded(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let mut database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
    for value in 1..=3 {
        database
            .insert(
                "t",
                ScalarMap::from([("a".to_string(), ScalarValue::Int(value))]),
            )
            .expect("insert");
    }
    (dir, database)
}

/// Hit/miss, invalidacion por escritura y TTL.
#[test]
fn query_cache_hits_misses_and_invalidation() {
    let (_dir, mut database) = seeded("qc");
    database.configure_query_cache(8, None);
    let query = "SELECT a FROM t";

    let first = database.execute_cached(query, 0).expect("miss");
    assert_eq!(first.len(), 3);
    assert_eq!(database.query_cache_stats().misses, 1);
    assert_eq!(database.query_cache_stats().hits, 0);

    let second = database.execute_cached(query, 0).expect("hit");
    assert_eq!(second, first);
    assert_eq!(database.query_cache_stats().hits, 1);

    // Una escritura invalida el cache (via invalidate_semantic_cache).
    database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(4))]),
        )
        .expect("insert");
    let third = database.execute_cached(query, 0).expect("miss tras write");
    assert_eq!(third.len(), 4);
    assert_eq!(database.query_cache_stats().misses, 2);

    database.invalidate_query_cache();
    assert_eq!(database.query_cache_stats().hits, 1);
}

/// TTL: una entrada vieja no se sirve.
#[test]
fn query_cache_respects_ttl() {
    let (_dir, mut database) = seeded("qc_ttl");
    database.configure_query_cache(8, Some(100));
    let query = "SELECT a FROM t";
    assert_eq!(
        database.execute_cached(query, 1_000).expect("miss").len(),
        3
    );
    assert_eq!(database.execute_cached(query, 1_050).expect("hit").len(), 3);
    assert_eq!(
        database.execute_cached(query, 2_000).expect("expira").len(),
        3
    );
    let stats = database.query_cache_stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.expired, 1);
    assert_eq!(stats.misses, 2);
}
