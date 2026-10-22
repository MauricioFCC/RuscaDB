//! Tests de aceptación de `JOIN` con `WHERE`/`ORDER BY` (SPEC-0108).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 64)).expect("apertura de la base");
    (dir, database)
}

fn create_tables(database: &mut Database) {
    database
        .create_table(
            "authors",
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
        .expect("create authors");
    database
        .create_table(
            "books",
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "author_id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "title".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create books");
}

fn ins_author(db: &mut Database, id: i64, name: &str) {
    db.insert(
        "authors",
        ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("name".to_string(), ScalarValue::Text(name.to_string())),
        ]),
    )
    .expect("ins author");
}
fn ins_book(db: &mut Database, id: i64, aid: i64, title: &str) {
    db.insert(
        "books",
        ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("author_id".to_string(), ScalarValue::Int(aid)),
            ("title".to_string(), ScalarValue::Text(title.to_string())),
        ]),
    )
    .expect("ins book");
}

#[test] // @spec AC-0108-01
fn test_ac_0108_01_join_where_filters_rows() {
    let (_dir, mut db) = open_test_db("ac0108_01");
    create_tables(&mut db);
    ins_author(&mut db, 1, "Ana");
    ins_author(&mut db, 2, "Bob");
    ins_book(&mut db, 1, 1, "A1");
    ins_book(&mut db, 2, 2, "B1");
    ins_book(&mut db, 3, 1, "A2");

    let rows = db.execute("SELECT authors.name, books.title FROM authors JOIN books ON authors.id = books.author_id WHERE name = 'Ana'").expect("query");
    assert_eq!(rows.len(), 2);
}

#[test] // @spec AC-0108-02
fn test_ac_0108_02_join_order_by_desc() {
    let (_dir, mut db) = open_test_db("ac0108_02");
    create_tables(&mut db);
    ins_author(&mut db, 1, "Ana");
    ins_author(&mut db, 2, "Bob");
    ins_book(&mut db, 1, 1, "A1");
    ins_book(&mut db, 2, 2, "B1");

    let rows = db.execute("SELECT authors.name FROM authors JOIN books ON authors.id = books.author_id ORDER BY name DESC").expect("query");
    assert_eq!(rows.len(), 2);
    // first should be Bob
    let first = match rows[0].get("authors.name").or_else(|| rows[0].get("name")) {
        Some(ScalarValue::Text(t)) => t.clone(),
        _ => String::new(),
    };
    assert_eq!(first, "Bob");
}
