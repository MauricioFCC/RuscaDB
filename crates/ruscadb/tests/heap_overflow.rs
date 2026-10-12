//! Tests de aceptación de overflow pages del heap (SPEC-0074).
//!
//! Cubre AC-0074-01..05: round-trip de filas que superan `MAX_ROW_BYTES`
//! (4090 B) mediante una cadena de páginas de desbordamiento, su liberación en
//! `update`/`delete` y la persistencia tras reabrir la base.
//!
//! Clave de observabilidad: el `doc` JSON no viaja en `Row`, así que el
//! round-trip se verifica con un escalar `Text` (columna `name`) mayor que
//! `MAX_ROW_BYTES`; `SELECT * FROM items` devuelve el texto íntegro.
//!
//! Estos tests son RED por diseño: hoy `serialize_row` rechaza filas mayores
//! que `MAX_ROW_BYTES` con `InvalidConfig`, por lo que las inserciones grandes
//! fallan hasta que exista el camino de overflow.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, ScalarMap, ScalarValue};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Tope nominal de una fila base en el heap (B): por encima arranca el overflow.
///
/// Referencia del diseño SPEC-0074; los fixtures deben superarlo.
const MAX_ROW_BYTES: usize = 4090;

/// Texto pequeño que cabe holgadamente en una página (tras el update).
const SMALL_TEXT: &str = "corto";

/// Longitud de un texto que exige **una** página de overflow (~6 KB > 4090 B).
const ONE_PAGE_BYTES: usize = 6_000;

/// Longitud de un texto que exige **varias** páginas de overflow (~12 KB).
const TWO_PAGES_BYTES: usize = 12_000;

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
    let database =
        Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura de la base");
    (dir, database)
}

/// Crea `items(id INT, name TEXT)`.
///
/// Args:
///     database: Base destino.
///     table: Nombre de la tabla a crear.
fn create_items(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
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
        .expect("create_table");
}

/// Construye escalares `(id, name)`.
///
/// Args:
///     id: Valor entero de la columna `id`.
///     name: Valor textual de la columna `name`.
///
/// Returns:
///     El mapa de escalares listo para un registro.
fn scalars(id: i64, name: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ])
}

/// Genera un texto de `bytes` caracteres `'A'` (payload de desbordamiento).
///
/// Args:
///     bytes: Longitud del texto en bytes/caracteres.
///
/// Returns:
///     La cadena `"A"` repetida `bytes` veces.
fn big_text(bytes: usize) -> String {
    "A".repeat(bytes)
}

/// Extrae el valor entero de una columna `Int` de una fila.
///
/// Args:
///     row: Fila observada.
///     column: Nombre de la columna entera.
///
/// Returns:
///     El `i64` almacenado.
fn int_of(row: &Row, column: &str) -> i64 {
    match row.get(column) {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en '{column}', se obtuvo {other:?}"),
    }
}

/// Extrae el valor de texto de una columna de una fila.
///
/// Args:
///     row: Fila observada.
///     column: Nombre de la columna textual.
///
/// Returns:
///     El `String` almacenado.
fn text_of(row: &Row, column: &str) -> String {
    match row.get(column) {
        Some(ScalarValue::Text(value)) => value.clone(),
        other => panic!("se esperaba Text en '{column}', se obtuvo {other:?}"),
    }
}

/// Localiza la fila con el `id` escalar dado.
///
/// Args:
///     rows: Filas devueltas por un `SELECT`.
///     id: Valor buscado en la columna `id`.
///
/// Returns:
///     Una referencia a la fila encontrada.
fn find_row_by_id(rows: &[Row], id: i64) -> &Row {
    rows.iter()
        .find(|row| int_of(row, "id") == id)
        .unwrap_or_else(|| {
            panic!(
                "no se encontró la fila con id = {id}; filas: {}",
                rows.len()
            )
        })
}

/// AC-0074-01 — round-trip de una fila con una única página de overflow.
#[test]
// @spec AC-0074-01
fn test_ac_0074_01_single_overflow_page_roundtrip() {
    let (_dir, mut database) = open_test_db("ac74_01");
    create_items(&mut database, "items");

    let big = big_text(ONE_PAGE_BYTES);
    assert!(
        big.len() > MAX_ROW_BYTES,
        "el fixture debe superar MAX_ROW_BYTES: {} <= {MAX_ROW_BYTES}",
        big.len()
    );

    database
        .insert("items", scalars(1, &big))
        .expect("insert de una fila con overflow (una pagina)");

    let rows = database.execute("SELECT * FROM items").expect("select");
    assert_eq!(rows.len(), 1, "el scan debe devolver la fila insertada");
    assert_eq!(
        text_of(find_row_by_id(&rows, 1), "name"),
        big,
        "round-trip integro del texto con overflow"
    );
}

/// AC-0074-02 — round-trip íntegro byte a byte de una fila multi-página.
#[test]
// @spec AC-0074-02
fn test_ac_0074_02_multi_page_overflow_roundtrip() {
    let (_dir, mut database) = open_test_db("ac74_02");
    create_items(&mut database, "items");

    let big = big_text(TWO_PAGES_BYTES);

    database
        .insert("items", scalars(1, &big))
        .expect("insert de una fila con overflow multi-pagina");

    let rows = database
        .execute("SELECT * FROM items WHERE id = 1")
        .expect("select por id");
    assert_eq!(rows.len(), 1, "solo existe una fila");
    assert_eq!(
        text_of(&rows[0], "name"),
        big,
        "round-trip byte a byte del texto multi-pagina"
    );
}

/// AC-0074-03 — un `UPDATE` que encoge la fila libera la cadena de overflow.
#[test]
// @spec AC-0074-03
fn test_ac_0074_03_update_releases_overflow() {
    let (_dir, mut database) = open_test_db("ac74_03");
    create_items(&mut database, "items");

    let big = big_text(ONE_PAGE_BYTES);
    database
        .insert("items", scalars(1, &big))
        .expect("insert inicial con overflow");

    database
        .execute("UPDATE items SET name = 'corto' WHERE id = 1")
        .expect("update a un valor que cabe en una pagina");

    let rows = database
        .execute("SELECT * FROM items")
        .expect("select tras update");
    assert_eq!(rows.len(), 1, "la fila sigue existiendo tras el update");
    assert_eq!(
        text_of(find_row_by_id(&rows, 1), "name"),
        SMALL_TEXT,
        "el nuevo valor se lee correctamente"
    );

    // La cadena liberada debe poder reutilizarse por otra fila grande.
    let second = "B".repeat(ONE_PAGE_BYTES);
    database
        .insert("items", scalars(2, &second))
        .expect("insert de una segunda fila grande reutilizando la cadena");

    let rows = database
        .execute("SELECT * FROM items")
        .expect("select final");
    assert_eq!(rows.len(), 2, "deben convivir ambas filas");
    assert_eq!(
        text_of(find_row_by_id(&rows, 1), "name"),
        SMALL_TEXT,
        "la fila actualizada permanece correcta"
    );
    assert_eq!(
        text_of(find_row_by_id(&rows, 2), "name"),
        second,
        "la segunda fila grande se lee integra"
    );
}

/// AC-0074-04 — un `DELETE` retira la fila y libera su cadena de overflow.
#[test]
// @spec AC-0074-04
fn test_ac_0074_04_delete_releases_overflow() {
    let (_dir, mut database) = open_test_db("ac74_04");
    create_items(&mut database, "items");

    let big = big_text(ONE_PAGE_BYTES);
    let id = database
        .insert("items", scalars(1, &big))
        .expect("insert de una fila con overflow");

    assert!(
        database
            .delete("items", &id)
            .expect("delete de fila con overflow"),
        "el delete debe reportar una fila borrada"
    );

    let rows = database
        .execute("SELECT * FROM items")
        .expect("select tras delete");
    assert!(
        rows.is_empty(),
        "la fila borrada no debe aparecer en el scan; filas: {}",
        rows.len()
    );

    // La cadena liberada por el delete debe quedar reutilizable.
    let revived = "C".repeat(ONE_PAGE_BYTES);
    database
        .insert("items", scalars(2, &revived))
        .expect("reinsert de una fila grande tras liberar la cadena");
    let rows = database
        .execute("SELECT * FROM items")
        .expect("select tras reinsert");
    assert_eq!(rows.len(), 1, "solo la fila reinsertada debe existir");
    assert_eq!(
        text_of(find_row_by_id(&rows, 2), "name"),
        revived,
        "la fila reinsertada se lee integra"
    );
}

/// AC-0074-05 — la fila con overflow sobrevive al cierre y reapertura.
#[test]
// @spec AC-0074-05
fn test_ac_0074_05_overflow_survives_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac74_05.db");
    let big = big_text(ONE_PAGE_BYTES);

    {
        let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
        create_items(&mut database, "items");
        database
            .insert("items", scalars(1, &big))
            .expect("insert de una fila con overflow");
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("reapertura");
    let rows = database
        .execute("SELECT * FROM items WHERE id = 1")
        .expect("select tras reopen");
    assert_eq!(rows.len(), 1, "la fila sobrevive al replay/WAL");
    assert_eq!(
        text_of(&rows[0], "name"),
        big,
        "el texto con overflow se lee integro tras reabrir"
    );
}
