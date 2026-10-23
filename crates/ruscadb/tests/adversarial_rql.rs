//! Tests adversariales de RQL: inyeccion, input malformado, valores limite.
//!
//! Cada test verifica que el parser/executor no paniquea y devuelve un
//! Result (Ok o Err) ante cualquier entrada, incluyendo ataques.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarValue};

fn open_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(format!("{tag}.db"));
    let db = Database::open(DbConfig::new(&path, 64)).expect("open");
    (dir, db)
}

fn setup(db: &mut Database) {
    db.create_table(
        "t",
        vec![
            ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            },
            ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            },
        ],
    )
    .expect("create");
    db.insert(
        "t",
        ruscadb::ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(1)),
            ("name".to_string(), ScalarValue::Text("alice".to_string())),
        ]),
    )
    .expect("insert");
}

/// El parser no paniquea ante input arbitrario (fuzz-like).
#[test]
fn test_adv_rql_arbitrary_inputs_no_panic() {
    let inputs = [
        "",
        "   ",
        ";",
        "SELECT",
        "SELECT *",
        "SELECT * FROM",
        "SELECT * FROM t WHERE",
        "SELECT * FROM t WHERE 1=1; DROP TABLE t;",
        "INSERT INTO t VALUES (1); DELETE FROM t;",
        "SELECT * FROM t WHERE name = 'it''s'",
        "SELECT * FROM t WHERE name = '\"; DROP TABLE t; --'",
        "SELECT * FROM t WHERE name = '\x00\x01\x02'",
        "SELECT * FROM t ORDER BY name ASC LIMIT 999999999999999999999",
        "SELECT * FROM t WHERE id = -9223372036854775808",
        "SELECT * FROM t WHERE id = 9223372036854775807",
        "SELECT * FROM t WHERE name LIKE '%'",
        "SELECT * FROM t /* comment */ WHERE id = 1",
        "SELECT * FROM t WHERE id IN (1,2,3)",
        "SELECT * FROM t WHERE id BETWEEN 1 AND 2",
        "SELECT * FROM nonexistent_table",
        "SELECT * FROM t WHERE id = 1 UNION SELECT * FROM t",
        "SELECT * FROM t; SELECT * FROM t",
        "EXPLAIN SELECT * FROM t",
        "SELECT * FROM t WHERE name = 'a' OR name = 'b'",
        "SELECT * FROM t ORDER BY name DESC LIMIT 0 OFFSET 0",
        "SELECT 1+1",
        "SELECT * FROM t WHERE name = ''''",
        "SELECT * FROM t WHERE id = 1.0",
        "SELECT * FROM t WHERE id = 1e308",
        "SELECT * FROM t WHERE id = NaN",
        "SELECT * FROM t WHERE id = Infinity",
    ];
    let (_dir, mut db) = open_db("adv_rql");
    setup(&mut db);
    for input in &inputs {
        // No debe paniquear; Ok o Err son ambos validos.
        let _ = db.execute(input);
    }
}

/// Inyeccion SQL: un literal con comillas no debe ejecutar DML adicional.
#[test]
fn test_adv_rql_injection_literal() {
    let (_dir, mut db) = open_db("adv_inj");
    setup(&mut db);
    // Intento de inyeccion: el nombre contiene comillas y punto y coma.
    let malicious = "O'Brien; DROP TABLE t; --";
    db.execute(&format!(
        "INSERT INTO t (id, name) VALUES (2, '{}')",
        malicious.replace('\'', "''")
    ))
    .expect("insert con comillas escapadas");
    // La tabla sigue existiendo con 2 filas.
    let rows = db.execute("SELECT * FROM t").expect("select");
    assert_eq!(rows.len(), 2, "la inyeccion no debe borrar la tabla");
}

/// Valores limite de enteros.
#[test]
fn test_adv_rql_integer_boundaries() {
    let (_dir, mut db) = open_db("adv_int");
    setup(&mut db);
    // i64::MAX
    let r = db.execute("INSERT INTO t (id, name) VALUES (9223372036854775807, 'max')");
    // Puede ser Ok o Err, pero no panic.
    let _ = r;
    // i64::MIN
    let r = db.execute("INSERT INTO t (id, name) VALUES (-9223372036854775808, 'min')");
    let _ = r;
}

/// Overflow de string muy largo.
#[test]
fn test_adv_rql_long_string() {
    let (_dir, mut db) = open_db("adv_long");
    setup(&mut db);
    let long_name = "a".repeat(10_000);
    let sql = format!("INSERT INTO t (id, name) VALUES (2, '{}')", long_name);
    let _ = db.execute(&sql);
    // No panic; la base sigue operativa.
    let _ = db.execute("SELECT * FROM t");
}

/// Deep nesting en expresiones WHERE.
#[test]
fn test_adv_rql_deep_nesting() {
    let (_dir, mut db) = open_db("adv_deep");
    setup(&mut db);
    // Expresion profundamente anidada.
    let mut expr = "id = 1".to_string();
    for _ in 0..50 {
        expr = format!("({expr} AND id = 1)");
    }
    let sql = format!("SELECT * FROM t WHERE {}", expr);
    let _ = db.execute(&sql);
}

/// Unicode en identificadores y valores.
#[test]
fn test_adv_rql_unicode() {
    let (_dir, mut db) = open_db("adv_unicode");
    db.create_table(
        "unicode_t",
        vec![
            ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            },
            ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            },
        ],
    )
    .expect("create");
    let _ = db.execute("INSERT INTO unicode_t (id, name) VALUES (1, 'café')");
    let _ = db.execute("INSERT INTO unicode_t (id, name) VALUES (2, '日本語')");
    let _ = db.execute("INSERT INTO unicode_t (id, name) VALUES (3, '🎉')");
    let rows = db.execute("SELECT * FROM unicode_t").expect("select");
    assert_eq!(rows.len(), 3);
}

/// DML con valores NULL.
#[test]
fn test_adv_rql_null_handling() {
    let (_dir, mut db) = open_db("adv_null");
    setup(&mut db);
    let _ = db.execute("INSERT INTO t (id, name) VALUES (2, NULL)");
    let rows = db.execute("SELECT * FROM t WHERE name IS NULL");
    // Puede ser Ok o Err, pero no panic.
    let _ = rows;
    let _ = db.execute("SELECT * FROM t WHERE name = NULL");
}
