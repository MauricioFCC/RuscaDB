//! SPEC-0203 — caché de planes acotada con invalidación por época.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Crea `t(k Int)` con una fila.
fn open(dir: &std::path::Path) -> Database {
    let path = dir.join("plan-cache.db");
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "k".to_string(),
            col_type: ColumnType::Int,
        }],
    )
    .expect("create");
    db.insert(
        "t",
        ScalarMap::from([("k".to_string(), ScalarValue::Int(1))]),
    )
    .expect("insert");
    db
}

/// AC-0203-01 — la segunda ejecución reutiliza el plan (hit) y el resultado es
/// idéntico.
#[test] // @spec AC-0203-01
fn test_ac_0203_01_plan_cache_hit() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    let sql = "SELECT * FROM t WHERE k = 1";

    let first = db.execute(sql).expect("first");
    let (hits, misses, entries) = db.plan_cache_stats();
    assert_eq!((hits, misses, entries), (0, 1, 1));
    assert_eq!(first.len(), 1);

    let second = db.execute(sql).expect("second");
    let (hits, misses, entries) = db.plan_cache_stats();
    assert_eq!((hits, misses, entries), (1, 1, 1));
    assert_eq!(second, first, "mismo resultado con plan cacheado");
}

/// AC-0203-02 — INSERT/ANALYZE/DDL invalidan el plan y la siguiente consulta
/// vuelve a planificar con el estado actual.
#[test] // @spec AC-0203-02
fn test_ac_0203_02_plan_cache_invalidates() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    let sql = "SELECT * FROM t WHERE k = 1";
    db.execute(sql).expect("seed");
    db.execute(sql).expect("hit");

    db.insert(
        "t",
        ScalarMap::from([("k".to_string(), ScalarValue::Int(1))]),
    )
    .expect("insert");
    let rows = db.execute(sql).expect("after insert");
    assert_eq!(rows.len(), 2, "ve la fila nueva");
    let (_, misses, _) = db.plan_cache_stats();
    assert_eq!(misses, 2, "INSERT invalida la caché");

    db.analyze("t").expect("analyze");
    db.execute(sql).expect("after analyze");
    let (_, misses, _) = db.plan_cache_stats();
    assert_eq!(misses, 3, "ANALYZE invalida la caché");

    db.create_index("t", "k").expect("index");
    db.execute(sql).expect("after ddl");
    let (_, misses, _) = db.plan_cache_stats();
    assert_eq!(misses, 4, "DDL invalida la caché");
}

/// AC-0203-03 — la caché está acotada (LRU) y no crece sin límite.
#[test] // @spec AC-0203-03
fn test_ac_0203_03_plan_cache_is_bounded() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    for value in 0..100 {
        db.execute(&format!("SELECT * FROM t WHERE k = {value}"))
            .expect("query");
    }
    let (hits, misses, entries) = db.plan_cache_stats();
    assert_eq!(hits, 0);
    assert_eq!(misses, 100);
    assert!(
        entries <= 64,
        "la caché debe estar acotada, tiene {entries}"
    );
}
