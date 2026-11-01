//! Aceptación SQL real de SPEC-0161 (expresiones RQL).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Base con `t(a INT, b INT, name TEXT)` incluyendo una fila NULL.
fn seed(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(format!("{tag}.db"));
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "t",
            vec![
                ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "b".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create");
    for (a, b, name) in [
        (ScalarValue::Int(1), ScalarValue::Int(10), "origen"),
        (ScalarValue::Int(2), ScalarValue::Int(20), "cerca"),
        (ScalarValue::Int(3), ScalarValue::Int(30), "lejos"),
        (ScalarValue::Null, ScalarValue::Null, "nulo"),
    ] {
        database
            .insert(
                "t",
                ScalarMap::from([
                    ("a".to_string(), a),
                    ("b".to_string(), b),
                    ("name".to_string(), ScalarValue::Text(name.to_string())),
                ]),
            )
            .expect("insert");
    }
    (dir, database)
}

/// AC-0161-02: `IN`/`NOT IN` con semántica NULL.
#[test] // @spec AC-0161-02
fn test_ac_0161_02_in_list_sql() {
    let (_dir, mut database) = seed("in");
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a IN (1, 3)")
            .expect("in")
            .len(),
        2
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a NOT IN (1, 3)")
            .expect("not in")
            .len(),
        1
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a IN (1, NULL)")
            .expect("in null")
            .len(),
        1,
        "NULL en la lista no debe volverse TRUE"
    );
}

/// AC-0161-03: `BETWEEN`/`NOT BETWEEN` inclusivo.
#[test] // @spec AC-0161-03
fn test_ac_0161_03_between_sql() {
    let (_dir, mut database) = seed("between");
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a BETWEEN 1 AND 2")
            .expect("between")
            .len(),
        2
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a NOT BETWEEN 1 AND 2")
            .expect("not between")
            .len(),
        1
    );
}

/// LIKE/ILIKE con `%`/`_` y negación.
#[test] // @spec AC-0161-02
fn test_ac_0161_02_like_sql() {
    let (_dir, mut database) = seed("like");
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE name LIKE 'or%'")
            .expect("like")
            .len(),
        1
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE name ILIKE 'OR%'")
            .expect("ilike")
            .len(),
        1
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE name LIKE '_erca'")
            .expect("underscore")
            .len(),
        1
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE name NOT LIKE '%j%'")
            .expect("not like")
            .len(),
        3
    );
}

/// AC-0161-05: `COALESCE`/`NULLIF` en WHERE y agregados.
#[test] // @spec AC-0161-05
fn test_ac_0161_05_coalesce_sql() {
    let (_dir, mut database) = seed("coalesce");
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE COALESCE(b, 0) = 0")
            .expect("coalesce")
            .len(),
        1
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE NULLIF(a, 1) IS NULL")
            .expect("nullif")
            .len(),
        2
    );
    let rows = database
        .execute("SELECT SUM(COALESCE(b, 0)) AS total FROM t")
        .expect("sum coalesce");
    assert_eq!(rows[0].get("total"), Some(&ScalarValue::Int(60)));
}
