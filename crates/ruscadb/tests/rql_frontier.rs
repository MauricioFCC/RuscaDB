//! Aceptación SQL de las specs frontier: tablas derivadas, CTE, ventanas,
//! grafos y expresiones de sesión (SPEC-0162/0163/0165/0185/0186).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Abre una base temporal y crea `t(g INT, x INT)` con filas `(1,1)`, `(1,2)`,
/// `(2,3)`.
fn seeded(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(format!("{tag}.db"));
    let mut database = Database::open(DbConfig::new(&path, 128)).expect("open");
    database
        .create_table(
            "t",
            vec![
                ColumnDef {
                    name: "g".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "x".to_string(),
                    col_type: ColumnType::Int,
                },
            ],
        )
        .expect("create");
    for (g, x) in [(1i64, 1i64), (1, 2), (2, 3)] {
        database
            .insert(
                "t",
                ScalarMap::from([
                    ("g".to_string(), ScalarValue::Int(g)),
                    ("x".to_string(), ScalarValue::Int(x)),
                ]),
            )
            .expect("insert");
    }
    (dir, database)
}

/// `FROM (subquery) AS alias` consulta la tabla derivada (AC-0162-04).
#[test] // @spec AC-0162-04
fn test_ac_0162_04_derived_table_sql() {
    let (_dir, mut database) = seeded("derived");
    let rows = database
        .execute("SELECT x FROM (SELECT x FROM t WHERE x > 1) AS d ORDER BY x")
        .expect("derived table");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get("x"), Some(&ScalarValue::Int(2)));
    assert_eq!(rows[1].get("x"), Some(&ScalarValue::Int(3)));
}

/// `WITH ... AS (...)` materializa y consulta el CTE (AC-0163-01).
#[test] // @spec AC-0163-01
fn test_ac_0163_01_cte_sql() {
    let (_dir, mut database) = seeded("cte");
    let rows = database
        .execute("WITH filtered AS (SELECT x FROM t WHERE x > 1) SELECT x FROM filtered ORDER BY x")
        .expect("cte");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get("x"), Some(&ScalarValue::Int(2)));
    assert_eq!(rows[1].get("x"), Some(&ScalarValue::Int(3)));
}

/// `WITH RECURSIVE` recorre una jerarquía hasta punto fijo (AC-0163-02).
#[test] // @spec AC-0163-02
fn test_ac_0163_02_recursive_cte_sql() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("cte_recursive.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "nodes",
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "parent".to_string(),
                    col_type: ColumnType::Int,
                },
            ],
        )
        .expect("create");
    for (id, parent) in [
        (1i64, ScalarValue::Null),
        (2, ScalarValue::Int(1)),
        (3, ScalarValue::Int(2)),
    ] {
        database
            .insert(
                "nodes",
                ScalarMap::from([
                    ("id".to_string(), ScalarValue::Int(id)),
                    ("parent".to_string(), parent),
                ]),
            )
            .expect("insert");
    }
    let rows = database
        .execute(
            "WITH RECURSIVE tree AS (\
                SELECT id FROM nodes WHERE parent IS NULL \
                UNION ALL \
                SELECT id FROM nodes WHERE parent IN (SELECT id FROM tree)\
             ) SELECT id FROM tree ORDER BY id",
        )
        .expect("recursive cte");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].get("id"), Some(&ScalarValue::Int(1)));
    assert_eq!(rows[1].get("id"), Some(&ScalarValue::Int(2)));
    assert_eq!(rows[2].get("id"), Some(&ScalarValue::Int(3)));
}

/// `ROW_NUMBER() OVER (ORDER BY ...)` numera secuencialmente (AC-0165-01).
#[test] // @spec AC-0165-01
fn test_ac_0165_01_row_number_sql() {
    let (_dir, mut database) = seeded("window_row_number");
    let rows = database
        .execute("SELECT x, ROW_NUMBER() OVER (ORDER BY x) AS rn FROM t ORDER BY x")
        .expect("row_number");
    assert_eq!(rows.len(), 3);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.get("rn"), Some(&ScalarValue::Int(index as i64 + 1)));
    }
}

/// `SUM(x) OVER (PARTITION BY g)` repite la suma por partición (AC-0165-02).
#[test] // @spec AC-0165-02
fn test_ac_0165_02_sum_over_sql() {
    let (_dir, mut database) = seeded("window_sum");
    let rows = database
        .execute("SELECT g, x, SUM(x) OVER (PARTITION BY g) AS s FROM t ORDER BY x")
        .expect("sum over");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].get("s"), Some(&ScalarValue::Int(3)));
    assert_eq!(rows[1].get("s"), Some(&ScalarValue::Int(3)));
    assert_eq!(rows[2].get("s"), Some(&ScalarValue::Int(3)));
}

/// `LAG(x, 1) OVER (ORDER BY x)` devuelve la fila anterior (AC-0165-03).
#[test] // @spec AC-0165-03
fn test_ac_0165_03_lag_sql() {
    let (_dir, mut database) = seeded("window_lag");
    let rows = database
        .execute("SELECT x, LAG(x, 1) OVER (ORDER BY x) AS prev FROM t ORDER BY x")
        .expect("lag");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].get("prev"), Some(&ScalarValue::Null));
    assert_eq!(rows[1].get("prev"), Some(&ScalarValue::Int(1)));
    assert_eq!(rows[2].get("prev"), Some(&ScalarValue::Int(2)));
}

/// `RELATE` crea una arista idempotente (AC-0185-01).
#[test] // @spec AC-0185-01
fn test_ac_0185_01_relate_idempotent_sql() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("relate.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "person",
            vec![ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            }],
        )
        .expect("create");
    let alice = database
        .insert(
            "person",
            ScalarMap::from([("name".to_string(), ScalarValue::Text("alice".to_string()))]),
        )
        .expect("alice");
    let bob = database
        .insert(
            "person",
            ScalarMap::from([("name".to_string(), ScalarValue::Text("bob".to_string()))]),
        )
        .expect("bob");
    let query = format!("RELATE person:{alice}->knows->person:{bob}");
    let first = database.execute(&query).expect("relate");
    assert_eq!(first[0].get("affected"), Some(&ScalarValue::Int(1)));
    let second = database.execute(&query).expect("relate idempotente");
    assert_eq!(second[0].get("affected"), Some(&ScalarValue::Int(0)));
}

/// `->label->tabla` devuelve los vecinos con esa etiqueta (AC-0185-02).
#[test] // @spec AC-0185-02
fn test_ac_0185_02_graph_path_sql() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("graph_path.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "person",
            vec![ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            }],
        )
        .expect("create");
    let alice = database
        .insert(
            "person",
            ScalarMap::from([("name".to_string(), ScalarValue::Text("alice".to_string()))]),
        )
        .expect("alice");
    let bob = database
        .insert(
            "person",
            ScalarMap::from([("name".to_string(), ScalarValue::Text("bob".to_string()))]),
        )
        .expect("bob");
    database
        .execute(&format!("RELATE person:{alice}->knows->person:{bob}"))
        .expect("relate");
    let rows = database
        .execute("SELECT ->knows->person AS friend FROM person")
        .expect("graph path");
    assert_eq!(rows.len(), 2);
    let friends: Vec<&ScalarValue> = rows.iter().filter_map(|row| row.get("friend")).collect();
    assert!(
        friends
            .iter()
            .any(|value| **value == ScalarValue::Text(bob.to_string())),
        "bob debe aparecer como vecino de alice"
    );
}

/// Un path encadenado navega nivel a nivel (BFS) sin producto de grados
/// (AC-0185-03).
#[test] // @spec AC-0185-03
fn test_ac_0185_03_chained_path_sql() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("graph_chain.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "person",
            vec![ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            }],
        )
        .expect("create");
    let mut ids = Vec::new();
    for name in ["alice", "bob", "carol"] {
        ids.push(
            database
                .insert(
                    "person",
                    ScalarMap::from([("name".to_string(), ScalarValue::Text(name.to_string()))]),
                )
                .expect("insert"),
        );
    }
    database
        .execute(&format!(
            "RELATE person:{}->knows->person:{}",
            ids[0], ids[1]
        ))
        .expect("alice->bob");
    database
        .execute(&format!(
            "RELATE person:{}->knows->person:{}",
            ids[1], ids[2]
        ))
        .expect("bob->carol");
    let rows = database
        .execute("SELECT ->knows->person->knows->person AS friend FROM person WHERE name = 'alice'")
        .expect("chained path");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("friend"),
        Some(&ScalarValue::Text(ids[2].to_string()))
    );
}

/// `LET` + variable en `WHERE` dentro de un script (AC-0186-01).
#[test] // @spec AC-0186-01
fn test_ac_0186_01_let_variable_sql() {
    let (_dir, mut database) = seeded("script_let");
    let rows = database
        .execute_script("LET $min = 2; SELECT x FROM t WHERE x > $min ORDER BY x")
        .expect("script");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("x"), Some(&ScalarValue::Int(3)));
}

/// `IF ... THEN ... ELSE ... END` en proyección (AC-0186-02).
#[test] // @spec AC-0186-02
fn test_ac_0186_02_if_projection_sql() {
    let (_dir, mut database) = seeded("script_if");
    let rows = database
        .execute("SELECT x, IF x > 1 THEN 'up' ELSE 'down' END AS dir FROM t ORDER BY x")
        .expect("if projection");
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].get("dir"),
        Some(&ScalarValue::Text("down".to_string()))
    );
    assert_eq!(
        rows[1].get("dir"),
        Some(&ScalarValue::Text("up".to_string()))
    );
    assert_eq!(
        rows[2].get("dir"),
        Some(&ScalarValue::Text("up".to_string()))
    );
}

/// `RETURN` devuelve el valor de la expresión (AC-0186-03).
#[test] // @spec AC-0186-03
fn test_ac_0186_03_return_sql() {
    let (_dir, mut database) = seeded("script_return");
    let rows = database
        .execute_script("LET $answer = 42; RETURN $answer")
        .expect("return");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("value"), Some(&ScalarValue::Int(42)));
}

/// Variable no definida -> error accionable y sin fugas entre scripts
/// (AC-0186-04).
#[test] // @spec AC-0186-04
fn test_ac_0186_04_variable_isolation_sql() {
    let (_dir, mut database) = seeded("script_isolation");
    database
        .execute_script("LET $local = 1; SELECT x FROM t WHERE x > $local")
        .expect("primer script");
    let error = database
        .execute_script("SELECT x FROM t WHERE x > $local")
        .expect_err("variable no definida");
    assert!(
        format!("{error}").contains("$local"),
        "el error debe nombrar la variable: {error}"
    );
}
