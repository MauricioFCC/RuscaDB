//! SPEC-0200 — hash join O(n+m) para igualdad sin índice en el lado interior.

#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Crea una definición de columna.
fn column(name: &str, col_type: ColumnType) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type,
    }
}

/// Fila escalar con dos enteros.
fn ints(id: i64, key: i64) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("k".to_string(), ScalarValue::Int(key)),
    ])
}

/// Abre una base temporal con las tablas del caso pequeño.
fn open_small(dir: &std::path::Path) -> Database {
    let path = dir.join("hash-join.db");
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
    db.create_table(
        "l",
        vec![column("id", ColumnType::Int), column("k", ColumnType::Int)],
    )
    .expect("create l");
    db.create_table(
        "r",
        vec![column("id", ColumnType::Int), column("k", ColumnType::Int)],
    )
    .expect("create r");
    for (id, key) in [(1, 1), (2, 1), (3, 2)] {
        db.insert("l", ints(id, key)).expect("insert l");
    }
    db.insert(
        "l",
        ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(4)),
            ("k".to_string(), ScalarValue::Null),
        ]),
    )
    .expect("insert l null");
    for (id, key) in [(10, 1), (11, 2), (12, 2)] {
        db.insert("r", ints(id, key)).expect("insert r");
    }
    db.insert(
        "r",
        ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(13)),
            ("k".to_string(), ScalarValue::Null),
        ]),
    )
    .expect("insert r null");
    db
}

/// Convierte el resultado en pares `(l.id, r.id)` para comparar sin orden.
fn pairs(rows: &[ruscadb::Row]) -> BTreeSet<(i64, i64)> {
    rows.iter()
        .filter_map(|row| {
            let left = match row.get("l.id") {
                Some(ScalarValue::Int(value)) => *value,
                _ => return None,
            };
            let right = match row.get("r.id") {
                Some(ScalarValue::Int(value)) => *value,
                _ => return None,
            };
            Some((left, right))
        })
        .collect()
}

/// AC-0200-01 — INNER/LEFT/RIGHT/FULL producen las mismas filas que el oráculo
/// y `NULL` nunca empareja.
#[test] // @spec AC-0200-01
fn test_ac_0200_01_hash_join_outer_oracles() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open_small(dir.path());
    clean_db(&mut db);

    let inner = db
        .execute("SELECT l.id, r.id FROM l JOIN r ON l.k = r.k")
        .expect("inner");
    assert_eq!(
        pairs(&inner),
        BTreeSet::from([(1, 10), (2, 10), (3, 11), (3, 12)]),
        "inner join"
    );

    let left = db
        .execute("SELECT l.id, r.id FROM l LEFT JOIN r ON l.k = r.k")
        .expect("left");
    assert_eq!(left.len(), 5, "left join incluye l sin pareja");
    assert!(
        left.iter()
            .any(|row| row.get("r.id") == Some(&ScalarValue::Null))
    );

    let right = db
        .execute("SELECT l.id, r.id FROM l RIGHT JOIN r ON l.k = r.k")
        .expect("right");
    assert_eq!(right.len(), 5, "right join incluye r sin pareja");

    let full = db
        .execute("SELECT l.id, r.id FROM l FULL JOIN r ON l.k = r.k")
        .expect("full");
    assert_eq!(full.len(), 6, "full join incluye ambos lados sin pareja");
}

/// AC-0200-02 — un join de igualdad 10k × 10k sin índice termina muy por
/// debajo del umbral de un nested loop O(n·m).
#[test] // @spec AC-0200-02
fn test_ac_0200_02_hash_join_scales_linearly() {
    const ROWS: i64 = 10_000;
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("hash-join-scale.db");
    let mut db = Database::open(DbConfig::new(&path, 256)).expect("open");
    for table in ["a", "b"] {
        db.create_table(
            table,
            vec![column("id", ColumnType::Int), column("k", ColumnType::Int)],
        )
        .expect("create");
        db.begin();
        for value in 0..ROWS {
            db.insert(table, ints(value, value)).expect("insert");
        }
        db.commit().expect("commit");
    }

    let started = Instant::now();
    let rows = db
        .execute("SELECT a.id, b.id FROM a JOIN b ON a.k = b.k LIMIT 100000")
        .expect("join");
    let elapsed = started.elapsed();
    assert_eq!(rows.len() as i64, ROWS, "cada fila tiene una pareja");
    assert!(
        elapsed < Duration::from_secs(5),
        "join 10k×10k tardó {elapsed:?}; el nested loop O(n·m) no escala"
    );
}

/// Fuerza el catálogo a recargarse tras las inserciones.
fn clean_db(db: &mut Database) {
    // `execute` carga el catálogo por sí mismo; el helper existe para dejar
    // explícito que no compartimos estado entre consultas.
    let _ = db.execute("SELECT COUNT(*) FROM l").expect("count");
}
