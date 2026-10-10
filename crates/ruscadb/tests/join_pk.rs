//! INNER JOIN de dos tablas por PK (SPEC-0052, AC-0052-01..04).
//!
//! Cubre emparejados/huérfanos, sin coincidencias, tabla vacía y `ON`
//! no-equi, más un oráculo PBT (JOIN == producto cartesiano filtrado
//! ingenuo) y casos BVA (1-1, 1-N, huérfanos en ambos lados, `NULL`).

// El oráculo y los helpers usan `expect` (igual que el resto de tests de
// integración de la fachada; `allow-expect-in-tests` no alcanza a este target).
#![allow(clippy::expect_used)]

use proptest::prelude::*;
use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, ScalarMap, ScalarValue};

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura de la base");
    (dir, database)
}

/// Crea `authors(id INT, name TEXT)` + `books(id INT, author_id INT, title TEXT)`.
fn create_schema(database: &mut Database) {
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
fn insert_author(database: &mut Database, id: ScalarValue, name: &str) {
    let scalars = ScalarMap::from([
        ("id".to_string(), id),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ]);
    database.insert("authors", scalars).expect("insert author");
}

/// Inserta un libro `(id, author_id, title)`.
fn insert_book(database: &mut Database, id: i64, author_id: ScalarValue, title: &str) {
    let scalars = ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("author_id".to_string(), author_id),
        ("title".to_string(), ScalarValue::Text(title.to_string())),
    ]);
    database.insert("books", scalars).expect("insert book");
}

/// Semilla canónica: 3 autores (uno huérfano) + 3 libros (uno huérfano).
fn seed_canonical(database: &mut Database) {
    create_schema(database);
    insert_author(database, ScalarValue::Int(1), "ada");
    insert_author(database, ScalarValue::Int(2), "grace");
    insert_author(database, ScalarValue::Int(3), "sola");
    insert_book(database, 10, ScalarValue::Int(1), "engine");
    insert_book(database, 11, ScalarValue::Int(2), "compiler");
    insert_book(database, 12, ScalarValue::Int(99), "huerfano");
}

/// Consulta canónica del JOIN por PK.
const JOIN_QUERY: &str =
    "SELECT authors.name, books.title FROM authors JOIN books ON authors.id = books.author_id";

/// AC-0052-01 — el INNER JOIN devuelve solo las filas emparejadas.
#[test]
// @spec AC-0052-01
fn test_ac_0052_01_inner_join_matches() {
    let (_dir, mut database) = open_test_db("ac0052_01");
    seed_canonical(&mut database);

    let rows = database.execute(JOIN_QUERY).expect("join");
    assert_eq!(rows.len(), 2, "solo 2 parejas coinciden: {rows:?}");
    assert_eq!(
        rows[0].get("authors.name"),
        Some(&ScalarValue::Text("ada".to_string()))
    );
    assert_eq!(
        rows[0].get("books.title"),
        Some(&ScalarValue::Text("engine".to_string()))
    );
    assert_eq!(
        rows[1].get("authors.name"),
        Some(&ScalarValue::Text("grace".to_string()))
    );
    assert_eq!(
        rows[1].get("books.title"),
        Some(&ScalarValue::Text("compiler".to_string()))
    );
}

/// AC-0052-02 — sin coincidencias el JOIN devuelve 0 filas.
#[test]
// @spec AC-0052-02
fn test_ac_0052_02_no_match_empty() {
    let (_dir, mut database) = open_test_db("ac0052_02");
    create_schema(&mut database);
    insert_author(&mut database, ScalarValue::Int(1), "ada");
    insert_book(&mut database, 10, ScalarValue::Int(2), "nadie");

    let rows = database.execute(JOIN_QUERY).expect("join");
    assert!(rows.is_empty(), "sin coincidencias no hay filas: {rows:?}");
}

/// AC-0052-03 — con una tabla vacía el JOIN devuelve 0 filas sin panics.
#[test]
// @spec AC-0052-03
fn test_ac_0052_03_empty_table() {
    let (_dir, mut database) = open_test_db("ac0052_03");
    create_schema(&mut database);
    insert_author(&mut database, ScalarValue::Int(1), "ada");

    let rows = database.execute(JOIN_QUERY).expect("join");
    assert!(rows.is_empty(), "tabla interior vacía: {rows:?}");

    let (_dir, mut other) = open_test_db("ac0052_03b");
    create_schema(&mut other);
    insert_book(&mut other, 10, ScalarValue::Int(1), "engine");
    let rows = other.execute(JOIN_QUERY).expect("join");
    assert!(rows.is_empty(), "tabla exterior vacía: {rows:?}");
}

/// AC-0052-04 — el `ON` no-equi o multi-condición da error accionable.
#[test]
// @spec AC-0052-04
fn test_ac_0052_04_non_equi_errors() {
    let (_dir, mut database) = open_test_db("ac0052_04");
    seed_canonical(&mut database);

    for query in [
        "SELECT * FROM authors JOIN books ON authors.id > books.author_id",
        "SELECT * FROM authors JOIN books ON authors.id != books.author_id",
        "SELECT * FROM authors JOIN books ON authors.id = books.author_id AND authors.id = books.id",
    ] {
        let error = database.execute(query).expect_err("ON no soportado");
        let message = error.to_string();
        assert!(
            message.contains("JOIN") || message.contains("ON") || message.contains("igualdad"),
            "error accionable para {query:?}: {message}"
        );
    }
}

/// BVA 1-N — un autor con 3 libros produce 3 filas.
#[test]
fn test_ac_0052_bva_one_to_many() {
    let (_dir, mut database) = open_test_db("ac0052_1n");
    create_schema(&mut database);
    insert_author(&mut database, ScalarValue::Int(1), "ada");
    for (id, title) in [(10, "a"), (11, "b"), (12, "c")] {
        insert_book(&mut database, id, ScalarValue::Int(1), title);
    }

    let rows = database.execute(JOIN_QUERY).expect("join");
    assert_eq!(rows.len(), 3, "1-N produce 3 filas: {rows:?}");
}

/// BVA — las claves `NULL` nunca emparejan (semántica SQL).
#[test]
fn test_ac_0052_bva_null_keys_never_match() {
    let (_dir, mut database) = open_test_db("ac0052_null");
    create_schema(&mut database);
    insert_author(&mut database, ScalarValue::Null, "fantasma");
    insert_book(&mut database, 10, ScalarValue::Null, "nadie");

    let rows = database.execute(JOIN_QUERY).expect("join");
    assert!(rows.is_empty(), "NULL = NULL no empareja: {rows:?}");
}

/// BVA — `SELECT *` fusiona con columnas prefijadas `tabla.col`.
#[test]
fn test_ac_0052_bva_star_uses_prefixed_columns() {
    let (_dir, mut database) = open_test_db("ac0052_star");
    seed_canonical(&mut database);

    let rows = database
        .execute("SELECT * FROM authors JOIN books ON authors.id = books.author_id")
        .expect("join");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].get("authors.name"),
        Some(&ScalarValue::Text("ada".to_string()))
    );
    assert_eq!(
        rows[0].get("books.title"),
        Some(&ScalarValue::Text("engine".to_string()))
    );
    assert_eq!(rows[0].get("authors.id"), Some(&ScalarValue::Int(1)));
    assert!(
        !rows[0].contains_key("name"),
        "sin prefijo ambiguo: {:?}",
        rows[0].keys().collect::<Vec<_>>()
    );
}

/// BVA — columna no cualificada sin colisión resuelve; con colisión exige cualificar.
#[test]
fn test_ac_0052_bva_collision_requires_qualification() {
    let (_dir, mut database) = open_test_db("ac0052_amb");
    seed_canonical(&mut database);

    let rows = database
        .execute("SELECT title FROM authors JOIN books ON authors.id = books.author_id")
        .expect("join");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].get("title"),
        Some(&ScalarValue::Text("engine".to_string()))
    );

    let error = database
        .execute("SELECT id FROM authors JOIN books ON authors.id = books.author_id")
        .expect_err("columna ambigua");
    assert!(
        error.to_string().contains("id"),
        "el error nombra la columna ambigua: {error}"
    );
}

/// BVA — el `LIMIT` se aplica a las filas del JOIN.
#[test]
fn test_ac_0052_bva_limit_applies() {
    let (_dir, mut database) = open_test_db("ac0052_lim");
    seed_canonical(&mut database);

    let rows = database
        .execute("SELECT * FROM authors JOIN books ON authors.id = books.author_id LIMIT 1")
        .expect("join");
    assert_eq!(rows.len(), 1);
}

/// BVA — con índice secundario en la columna interior el resultado coincide.
#[test]
fn test_ac_0052_bva_index_lookup_matches_scan() {
    let (_dir, mut database) = open_test_db("ac0052_idx");
    seed_canonical(&mut database);
    let without_index = database.execute(JOIN_QUERY).expect("join sin índice");
    database
        .create_index("books", "author_id")
        .expect("create_index");
    let with_index = database.execute(JOIN_QUERY).expect("join con índice");
    assert_eq!(with_index, without_index);
}

/// El JOIN rechaza agregados y `ORDER BY` con error accionable (fuera de alcance).
#[test]
fn test_ac_0052_bva_unsupported_clauses_error() {
    let (_dir, mut database) = open_test_db("ac0052_unsup");
    seed_canonical(&mut database);

    let aggregated = database
        .execute("SELECT COUNT(*) FROM authors JOIN books ON authors.id = books.author_id")
        .expect_err("agregado con JOIN");
    assert!(aggregated.to_string().contains("JOIN"), "{aggregated}");

    let ordered = database
        .execute("SELECT * FROM authors JOIN books ON authors.id = books.author_id ORDER BY authors.name")
        .expect_err("ORDER BY con JOIN");
    assert!(ordered.to_string().contains("JOIN"), "{ordered}");
}

/// Ordena filas por su representación para comparar multiconjuntos.
fn sort_rows(rows: &mut [Row]) {
    rows.sort_by_key(|row| format!("{row:?}"));
}

proptest! {
    /// Oráculo: el JOIN == producto cartesiano filtrado ingenuo.
    #[test]
    fn prop_join_matches_naive_cartesian_filter(
        author_ids in prop::collection::vec(0i64..4, 0..8),
        book_keys in prop::collection::vec(0i64..4, 0..8),
    ) {
        let (_dir, mut database) = open_test_db("prop_join");
        create_schema(&mut database);
        for (index, id) in author_ids.iter().enumerate() {
            insert_author(&mut database, ScalarValue::Int(*id), &format!("a{index}"));
        }
        for (index, key) in book_keys.iter().enumerate() {
            insert_book(&mut database, index as i64, ScalarValue::Int(*key), &format!("t{index}"));
        }

        let mut actual = database
            .execute("SELECT * FROM authors JOIN books ON authors.id = books.author_id")
            .expect("join");
        let authors = database.execute("SELECT * FROM authors").expect("authors");
        let books = database.execute("SELECT * FROM books").expect("books");
        let mut expected: Vec<Row> = Vec::new();
        for left in &authors {
            for right in &books {
                if left.get("id") == right.get("author_id") {
                    let mut merged = Row::new();
                    for (column, value) in left {
                        merged.insert(format!("authors.{column}"), value.clone());
                    }
                    for (column, value) in right {
                        merged.insert(format!("books.{column}"), value.clone());
                    }
                    expected.push(merged);
                }
            }
        }
        sort_rows(&mut actual);
        sort_rows(&mut expected);
        prop_assert_eq!(actual, expected);
    }
}
