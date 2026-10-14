//! Tests de aceptación de `RIGHT`/`FULL`/`CROSS JOIN` (SPEC-0103).
//!
//! Cubre AC-0103-01 (RIGHT conserva las filas interiores sin contraparte),
//! AC-0103-02 (FULL conserva las de ambos lados) y AC-0103-03 (CROSS =
//! producto cartesiano). El parseo/canonización (AC-0103-04) vive en las
//! pruebas unitarias de `ruscadb-query`.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, ScalarMap, ScalarValue};

/// Abre una base temporal de pruebas con pool amplio.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 64)).expect("apertura de la base");
    (dir, database)
}

/// Crea `authors(id INT, name TEXT)` y `books(id INT, author_id INT, title TEXT)`.
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
        .expect("create_table authors");
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
        .expect("create_table books");
}

/// Siembra autores (uno sin libros) y libros (uno huérfano).
fn seed(database: &mut Database) {
    for (id, name) in [(1_i64, "ana"), (2, "luis"), (3, "sinlibro")] {
        database
            .insert(
                "authors",
                ScalarMap::from([
                    ("id".to_string(), ScalarValue::Int(id)),
                    ("name".to_string(), ScalarValue::Text(name.to_string())),
                ]),
            )
            .expect("insert author");
    }
    for (id, author_id, title) in [
        (10_i64, 1_i64, "A"),
        (11, 1, "B"),
        (12, 2, "C"),
        (13, 99, "Z"),
    ] {
        database
            .insert(
                "books",
                ScalarMap::from([
                    ("id".to_string(), ScalarValue::Int(id)),
                    ("author_id".to_string(), ScalarValue::Int(author_id)),
                    ("title".to_string(), ScalarValue::Text(title.to_string())),
                ]),
            )
            .expect("insert book");
    }
}

/// Cuenta las filas cuyo `authors.name` está a NULL.
fn authors_null(rows: &[Row]) -> usize {
    rows.iter()
        .filter(|row| row.get("authors.name") == Some(&ScalarValue::Null))
        .count()
}

/// Cuenta las filas cuyo `books.title` está a NULL.
fn books_null(rows: &[Row]) -> usize {
    rows.iter()
        .filter(|row| row.get("books.title") == Some(&ScalarValue::Null))
        .count()
}

/// AC-0103-01 — `RIGHT JOIN` conserva las filas interiores sin contraparte.
#[test] // @spec AC-0103-01
fn test_ac_0103_01_right_join_keeps_unmatched_inner() {
    let (_dir, mut database) = open_test_db("ac0103_01");
    create_tables(&mut database);
    seed(&mut database);

    let rows = database
        .execute("SELECT * FROM authors RIGHT JOIN books ON books.author_id = authors.id")
        .expect("right join");
    // 3 pares (ana×2, luis×1) + el libro huérfano (autor 99) con authors.* NULL.
    assert_eq!(rows.len(), 4, "RIGHT conserva el libro huérfano: {rows:?}");
    assert_eq!(
        authors_null(&rows),
        1,
        "el libro huérfano aparece con authors.* a NULL"
    );
    assert_eq!(
        books_null(&rows),
        0,
        "RIGHT no añade libros fantasma (el lado conservado es books)"
    );
}

/// AC-0103-02 — `FULL JOIN` conserva las filas sin contraparte de ambos lados.
#[test] // @spec AC-0103-02
fn test_ac_0103_02_full_join_keeps_both_sides() {
    let (_dir, mut database) = open_test_db("ac0103_02");
    create_tables(&mut database);
    seed(&mut database);

    let rows = database
        .execute("SELECT * FROM authors FULL JOIN books ON books.author_id = authors.id")
        .expect("full join");
    // 3 pares + sinlibro (books.* NULL) + libro huérfano (authors.* NULL) = 5.
    assert_eq!(rows.len(), 5, "FULL une ambos lados: {rows:?}");
    assert_eq!(authors_null(&rows), 1, "un libro huérfano (lado books)");
    assert_eq!(books_null(&rows), 1, "un autor sin libros (lado authors)");
}

/// AC-0103-03 — `CROSS JOIN` es el producto cartesiano de las filas visibles.
#[test] // @spec AC-0103-03
fn test_ac_0103_03_cross_join_is_cartesian() {
    let (_dir, mut database) = open_test_db("ac0103_03");
    create_tables(&mut database);
    seed(&mut database);

    let rows = database
        .execute("SELECT * FROM authors CROSS JOIN books")
        .expect("cross join");
    assert_eq!(rows.len(), 3 * 4, "3 autores × 4 libros = 12 filas");
    assert_eq!(
        authors_null(&rows) + books_null(&rows),
        0,
        "CROSS no produce columnas NULL (no hay emparejamiento)"
    );
}
