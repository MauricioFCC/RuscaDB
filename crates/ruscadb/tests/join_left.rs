//! Tests de aceptación de `LEFT JOIN` (SPEC-0099).
//!
//! Cubre AC-0099-01 (la fila exterior sin contraparte se conserva con las
//! columnas interiores a `NULL`, en las rutas indexada y no indexada) y
//! AC-0099-02 (diferencia exacta INNER vs LEFT). El parseo/canonización
//! (AC-0099-03) vive en las pruebas unitarias de `ruscadb-query`.

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

/// Inserta un autor `(id, name)`.
fn insert_author(database: &mut Database, id: i64, name: &str) {
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

/// Inserta un libro `(id, author_id, title)`.
fn insert_book(database: &mut Database, id: i64, author_id: i64, title: &str) {
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

/// Siembra autores (incluido uno sin libros y otro con `id` NULL) y libros.
fn seed(database: &mut Database) {
    insert_author(database, 1, "ana");
    insert_author(database, 2, "luis");
    insert_author(database, 3, "sinlibro");
    // Autor con clave NULL: nunca empareja, pero LEFT debe conservarlo.
    database
        .insert(
            "authors",
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Null),
                ("name".to_string(), ScalarValue::Text("sinid".to_string())),
            ]),
        )
        .expect("insert author sin id");
    insert_book(database, 10, 1, "A");
    insert_book(database, 11, 1, "B");
    insert_book(database, 12, 2, "C");
}

/// Cuenta las filas cuyo `authors.name` coincide con `name`.
fn count_author(rows: &[Row], name: &str) -> usize {
    rows.iter()
        .filter(|row| row.get("authors.name") == Some(&ScalarValue::Text(name.to_string())))
        .count()
}

/// Indica si `row` tiene `books.title` a NULL.
fn book_title_is_null(row: &Row) -> bool {
    row.get("books.title") == Some(&ScalarValue::Null)
}

/// AC-0099-01 — `LEFT JOIN` conserva la fila exterior sin contraparte.
#[test] // @spec AC-0099-01
fn test_ac_0099_01_left_join_keeps_unmatched_outer() {
    let (_dir, mut database) = open_test_db("ac0099_01");
    create_tables(&mut database);
    seed(&mut database);

    let query = "SELECT * FROM authors LEFT JOIN books ON books.author_id = authors.id";
    let rows = database.execute(query).expect("left join (barrido)");

    // ana aporta 2 libros, luis 1; sinlibro y sinid (clave NULL) 1 cada uno.
    assert_eq!(
        rows.len(),
        5,
        "LEFT conserva las 4 filas exteriores: {rows:?}"
    );
    assert_eq!(count_author(&rows, "ana"), 2, "ana tiene 2 libros");
    assert_eq!(count_author(&rows, "luis"), 1, "luis tiene 1 libro");
    assert_eq!(
        count_author(&rows, "sinlibro"),
        1,
        "sinlibro aparece una vez sin contraparte"
    );
    assert_eq!(
        count_author(&rows, "sinid"),
        1,
        "una clave exterior NULL también se conserva (nunca empareja)"
    );
    let unmatched: Vec<&Row> = rows
        .iter()
        .filter(|row| count_author(std::slice::from_ref(row), "sinlibro") == 1)
        .collect();
    assert!(
        book_title_is_null(unmatched[0]),
        "la fila sin contraparte debe traer books.* a NULL"
    );

    // Misma semántica por la ruta indexada (índice en la columna interior).
    database
        .create_index("books", "author_id")
        .expect("create_index books.author_id");
    let indexed = database.execute(query).expect("left join (indexado)");
    assert_eq!(
        indexed, rows,
        "LEFT JOIN por índice debe coincidir con el barrido"
    );
}

/// AC-0099-02 — diferencia exacta INNER vs LEFT (una fila sin contraparte).
#[test] // @spec AC-0099-02
fn test_ac_0099_02_inner_vs_left_difference() {
    let (_dir, mut database) = open_test_db("ac0099_02");
    create_tables(&mut database);
    seed(&mut database);

    let inner = database
        .execute("SELECT * FROM authors JOIN books ON books.author_id = authors.id")
        .expect("inner join");
    let left = database
        .execute("SELECT * FROM authors LEFT JOIN books ON books.author_id = authors.id")
        .expect("left join");

    assert_eq!(inner.len(), 3, "INNER empareja 3 pares");
    assert_eq!(
        left.len(),
        inner.len() + 2,
        "LEFT añade las dos filas exteriores sin contraparte (sinlibro y sinid)"
    );
    assert_eq!(
        count_author(&inner, "sinlibro"),
        0,
        "INNER omite la fila sin contraparte"
    );
    assert_eq!(
        count_author(&left, "sinlibro"),
        1,
        "LEFT incluye la fila sin contraparte"
    );
}
