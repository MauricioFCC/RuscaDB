//! SPEC-0201 — planner por coste y estadísticas de tabla.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Crea una columna.
fn column(name: &str, col_type: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type,
    }
}

/// Fila con dos enteros.
fn ints(id: i64, key: i64) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("k".to_string(), ScalarValue::Int(key)),
    ])
}

/// Crea `t(id Int, k Int)` y devuelve la base.
fn open_table(dir: &std::path::Path) -> Database {
    let path = dir.join("stats.db");
    let mut db = Database::open(DbConfig::new(&path, 256)).expect("open");
    db.create_table(
        "t",
        vec![column("id", ColumnType::Int), column("k", ColumnType::Int)],
    )
    .expect("create");
    db
}

/// AC-0201-01 — stats exactas contra el oráculo (filas, NULLs, distintos,
/// min y max) e histograma numérico.
#[test] // @spec AC-0201-01
fn test_ac_0201_01_stats_collect() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open_table(dir.path());
    db.begin();
    for value in [3, 1, 1, 2, 5] {
        db.insert("t", ints(value, value)).expect("insert");
    }
    db.insert(
        "t",
        ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(9)),
            ("k".to_string(), ScalarValue::Null),
        ]),
    )
    .expect("insert null");
    db.commit().expect("commit");

    let stats = db.analyze("t").expect("analyze");
    assert_eq!(stats.rows, 6);
    let column = stats.columns.get("k").expect("k");
    assert_eq!(column.nulls, 1);
    assert_eq!(column.distinct, 4, "1,2,3,5");
    assert_eq!(column.min, Some(ScalarValue::Int(1)));
    assert_eq!(column.max, Some(ScalarValue::Int(5)));
    assert!(!column.histogram.is_empty(), "histograma numérico");
    assert_eq!(column.histogram.iter().sum::<u64>(), 5);
    assert!(db.stats("t").is_some(), "stats frescas");
}

/// AC-0201-02 — selectividad de igualdad y rango con error ≤ 10 % frente a la
/// frecuencia empírica.
#[test] // @spec AC-0201-02
fn test_ac_0201_02_selectivity_estimate() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open_table(dir.path());
    db.begin();
    for value in 0..1_000 {
        db.insert("t", ints(value, value)).expect("insert");
    }
    db.commit().expect("commit");
    let stats = db.analyze("t").expect("analyze");

    let equality = stats.equality_selectivity("k", &ScalarValue::Int(500));
    assert!(
        (equality - 0.001).abs() <= 0.0001,
        "igualdad estimada {equality}, esperada 0.001"
    );

    let range = stats.range_selectivity("k", Some(250.0), Some(750.0));
    assert!(
        (range - 0.5).abs() <= 0.05,
        "rango estimado {range}, esperado ~0.5"
    );

    let out_of_range = stats.equality_selectivity("k", &ScalarValue::Int(9_999));
    assert_eq!(out_of_range, 0.0, "fuera de min/max no empareja");
}

/// AC-0201-03 — igualdad selectiva usa IndexScan; un predicado no selectivo
/// usa FullScan; EXPLAIN expone estimación, coste y estrategia.
#[test] // @spec AC-0201-03
fn test_ac_0201_03_plan_choice() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open_table(dir.path());
    db.begin();
    for value in 0..10_000 {
        db.insert("t", ints(value, value)).expect("insert");
    }
    db.commit().expect("commit");
    db.create_index("t", "k").expect("create index");
    db.analyze("t").expect("analyze");

    let selective = db
        .explain("EXPLAIN SELECT * FROM t WHERE k = 9999")
        .expect("explain selective");
    let text = selective.join("\n");
    assert!(text.contains("IndexScan(t.k)"), "{text}");
    assert!(text.contains("STRATEGY IndexScan"), "{text}");
    assert!(text.contains("STATS fresh"), "{text}");
    assert!(text.contains("ESTIMATED_ROWS"), "{text}");
    assert!(text.contains("COST"), "{text}");

    let scan = db
        .explain("EXPLAIN SELECT * FROM t WHERE k >= 0")
        .expect("explain scan");
    let text = scan.join("\n");
    assert!(text.contains("FullScan(t)"), "{text}");
    assert!(text.contains("STRATEGY SeqScan"), "{text}");
}

/// AC-0201-04 — join sin índice interior elige HashJoin; con índice y exterior
/// pequeño elige IndexNestedLoop; EXPLAIN declara la estrategia.
#[test] // @spec AC-0201-04
fn test_ac_0201_04_join_strategy_cost() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("join.db");
    let mut db = Database::open(DbConfig::new(&path, 256)).expect("open");
    for table in ["a", "b"] {
        db.create_table(
            table,
            vec![column("id", ColumnType::Int), column("k", ColumnType::Int)],
        )
        .expect("create");
    }
    db.begin();
    for value in 0..100 {
        db.insert("a", ints(value, value)).expect("insert a");
        db.insert("b", ints(value, value)).expect("insert b");
    }
    db.commit().expect("commit");
    db.analyze("a").expect("analyze a");
    db.analyze("b").expect("analyze b");

    let hash = db
        .explain("EXPLAIN SELECT * FROM a JOIN b ON a.k = b.k")
        .expect("explain hash");
    let text = hash.join("\n");
    assert!(text.contains("STRATEGY HashJoin"), "{text}");
    assert_eq!(
        db.execute("SELECT * FROM a JOIN b ON a.k = b.k")
            .expect("join")
            .len(),
        100
    );

    // Índice interior + exterior pequeño: index nested loop.
    db.begin();
    for value in 100..1_000 {
        db.insert("b", ints(value, value)).expect("insert b");
    }
    db.commit().expect("commit");
    db.create_index("b", "k").expect("index b.k");
    db.analyze("b").expect("analyze b");
    let nested = db
        .explain("EXPLAIN SELECT * FROM a JOIN b ON a.k = b.k")
        .expect("explain nested");
    let text = nested.join("\n");
    assert!(text.contains("STRATEGY IndexNestedLoop"), "{text}");
}

/// Las stats persisten y se invalidan al mutar la tabla.
#[test]
fn stats_persist_and_invalidate() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("persist.db");
    {
        let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
        db.create_table(
            "t",
            vec![column("id", ColumnType::Int), column("k", ColumnType::Int)],
        )
        .expect("create");
        db.insert("t", ints(1, 1)).expect("insert");
        db.analyze("t").expect("analyze");
        assert!(db.stats("t").is_some());
    }
    let mut reopened = Database::open(DbConfig::new(&path, 64)).expect("reopen");
    assert!(reopened.stats("t").is_some(), "stats persistidas");
    reopened.insert("t", ints(2, 2)).expect("insert");
    assert!(reopened.stats("t").is_none(), "mutación invalida stats");
}
