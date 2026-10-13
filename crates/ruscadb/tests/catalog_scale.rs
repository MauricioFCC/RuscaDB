//! Tests de aceptación del catálogo sin techo de 64 KiB (SPEC-0086).
//!
//! Cubre AC-0086-01..03: un catálogo que supera las 16 páginas reservadas
//! (64 KiB) paga páginas extra con el mismo asignador *bump* del heap (patrón
//! overflow de SPEC-0071) y sobrevive al cierre/reapertura; las páginas del
//! catálogo y del heap nunca colisionan; y el formato legacy (≤ 16 páginas,
//! SPEC-0031) sigue cargando sin migración.
//!
//! Clave de observabilidad: el superblock vive en la página 0 (magic `u64` +
//! versión `u32`), por lo que la versión `2` (formato paginado) frente a la `1`
//! (reservado legacy) se verifica leyendo la página 0 por la API pública.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`).
#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, PageId, ScalarMap, ScalarValue};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 256;

/// Nº de tablas anchas a crear: basta para superar 16 páginas de catálogo.
const TABLES_TO_OVERFLOW: usize = 40;

/// Columnas por tabla ancha (acelera el crecimiento del catálogo).
const COLUMNS_PER_TABLE: usize = 200;

/// Filas sembradas en la tabla de heap del test de no-colisión.
const ROWS_TO_SEED: usize = 6;

/// Versión del formato legacy (región reservada contigua, SPEC-0031).
const CATALOG_VERSION_RESERVED: u32 = 1;

/// Versión del formato paginado con cadena de overflow (SPEC-0086).
const CATALOG_VERSION_OVERFLOW: u32 = 2;

/// Nombre determinista de la tabla ancha `index`.
///
/// Args:
///     index: Índice de la tabla.
///
/// Returns:
///     El nombre `wide_{index:04}`.
fn wide_table_name(index: usize) -> String {
    format!("wide_{index:04}")
}

/// Esquema de `columns` columnas `Int` llamadas `c000..cNNN`.
///
/// Args:
///     columns: Número de columnas.
///
/// Returns:
///     El vector de definiciones de columna.
fn wide_columns(columns: usize) -> Vec<ColumnDef> {
    (0..columns)
        .map(|column| ColumnDef {
            name: format!("c{column:03}"),
            col_type: ColumnType::Int,
        })
        .collect()
}

/// Crea una tabla ancha (`COLUMNS_PER_TABLE` columnas) con el nombre dado.
///
/// Args:
///     database: Base destino.
///     name: Nombre de la tabla.
fn create_wide_table(database: &mut Database, name: &str) {
    database
        .create_table(name, wide_columns(COLUMNS_PER_TABLE))
        .expect("create_table ancha");
}

/// Lee la versión del superblock de la página 0.
///
/// Args:
///     database: Base abierta.
///
/// Returns:
///     La versión `u32` almacenada en `8..12`.
fn catalog_version(database: &mut Database) -> u32 {
    let page = database.read_page(PageId(0)).expect("página 0");
    u32::from_le_bytes([
        page.data()[8],
        page.data()[9],
        page.data()[10],
        page.data()[11],
    ])
}

/// Devuelve los `PageId` de la cadena de overflow del catálogo v2.
///
/// Args:
///     database: Base abierta.
///
/// Returns:
///     Las páginas de overflow enlazadas (vacío si el catálogo es legacy).
fn catalog_overflow_pages(database: &mut Database) -> Vec<u64> {
    if catalog_version(database) != CATALOG_VERSION_OVERFLOW {
        return Vec::new();
    }
    let first = database.read_page(PageId(0)).expect("página 0");
    let count = u32::from_le_bytes([
        first.data()[12],
        first.data()[13],
        first.data()[14],
        first.data()[15],
    ]) as usize;
    let mut current = u64::from_le_bytes([
        first.data()[16],
        first.data()[17],
        first.data()[18],
        first.data()[19],
        first.data()[20],
        first.data()[21],
        first.data()[22],
        first.data()[23],
    ]);
    let mut pages = Vec::with_capacity(count);
    for _ in 0..count {
        assert_ne!(current, 0, "la cadena de overflow no debe truncarse");
        pages.push(current);
        let page = database
            .read_page(PageId(current))
            .expect("página de overflow");
        current = u64::from_le_bytes([
            page.data()[0],
            page.data()[1],
            page.data()[2],
            page.data()[3],
            page.data()[4],
            page.data()[5],
            page.data()[6],
            page.data()[7],
        ]);
    }
    pages
}

/// Verifica que la tabla existe con `COLUMNS_PER_TABLE` columnas `Int`.
///
/// Args:
///     database: Base abierta.
///     name: Nombre de la tabla.
fn assert_wide_table_intact(database: &mut Database, name: &str) {
    let catalog = database.catalog().expect("catálogo");
    let table = catalog.get(name).expect("la tabla existe");
    assert_eq!(
        table.columns.len(),
        COLUMNS_PER_TABLE,
        "la tabla '{name}' debe conservar sus columnas"
    );
    for (index, column) in table.columns.iter().enumerate() {
        assert_eq!(column.name, format!("c{index:03}"));
        assert_eq!(column.col_type, ColumnType::Int);
    }
}

/// Escalares `(id, data)` para la tabla de heap.
///
/// Args:
///     id: Valor entero de la columna `id`.
///     data: Valor textual de la columna `data`.
///
/// Returns:
///     El mapa de escalares listo para insertar.
fn item_scalars(id: i64, data: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("data".to_string(), ScalarValue::Text(data.to_string())),
    ])
}

/// AC-0086-01 — un catálogo que supera 16 páginas persiste y recarga completo.
///
/// Crea suficientes tablas anchas para rebasar la región reservada, comprueba
/// que el formato pasó a v2 (paginado), cierra, reabre y verifica que todas las
/// tablas y columnas se recargan íntegras.
#[test]
// @spec AC-0086-01
fn test_ac_0086_01_catalog_grows_beyond_reserved_pages() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac86_01.db");
    {
        let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
        for index in 0..TABLES_TO_OVERFLOW {
            create_wide_table(&mut database, &wide_table_name(index));
        }
        assert_eq!(
            catalog_version(&mut database),
            CATALOG_VERSION_OVERFLOW,
            "el catálogo debe haber superado la región reservada (formato v2)"
        );
        assert!(
            !catalog_overflow_pages(&mut database).is_empty(),
            "el catálogo ampliado debe tener al menos una página de overflow"
        );
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("reapertura");
    let names = database.tables().expect("tablas");
    assert_eq!(
        names.len(),
        TABLES_TO_OVERFLOW,
        "todas las tablas deben recargarse"
    );
    for index in 0..TABLES_TO_OVERFLOW {
        let name = wide_table_name(index);
        assert!(
            names.contains(&name),
            "falta la tabla '{name}' tras reabrir"
        );
        assert_wide_table_intact(&mut database, &name);
    }
}

/// AC-0086-02 — las páginas del catálogo ampliado y del heap nunca colisionan.
///
/// Siembra filas en una tabla de heap antes y después de que el catálogo crezca
/// a formato paginado; tras reabrir comprueba que las filas y las tablas siguen
/// intactas y que ninguna página del heap coincide con una página de overflow ni
/// con la región reservada `0..16`.
#[test]
// @spec AC-0086-02
fn test_ac_0086_02_heap_and_catalog_pages_never_collide() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac86_02.db");
    let mut expected: Vec<(i64, String)> = Vec::new();
    {
        let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
        database
            .create_table(
                "items",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        col_type: ColumnType::Int,
                    },
                    ColumnDef {
                        name: "data".to_string(),
                        col_type: ColumnType::Text,
                    },
                ],
            )
            .expect("create_table items");

        // Filas antes de que el catálogo crezca.
        for id in 0..ROWS_TO_SEED as i64 {
            let data = format!("antes-{id}");
            database
                .insert("items", item_scalars(id, &data))
                .expect("insert previo");
            expected.push((id, data));
        }

        // El catálogo crece a formato paginado.
        for index in 0..TABLES_TO_OVERFLOW {
            create_wide_table(&mut database, &wide_table_name(index));
        }
        assert_eq!(
            catalog_version(&mut database),
            CATALOG_VERSION_OVERFLOW,
            "el catálogo debe estar paginado tras crear las tablas anchas"
        );

        // Filas después del crecimiento: deben ir a páginas de heap nuevas.
        for id in ROWS_TO_SEED as i64..(ROWS_TO_SEED as i64 * 2) {
            let data = format!("despues-{id}");
            database
                .insert("items", item_scalars(id, &data))
                .expect("insert posterior");
            expected.push((id, data));
        }
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("reapertura");

    // Integridad del heap: todas las filas siguen legibles.
    let rows = database.execute("SELECT * FROM items").expect("select");
    assert_eq!(
        rows.len(),
        expected.len(),
        "deben recargarse todas las filas"
    );
    for (id, data) in &expected {
        let row = rows
            .iter()
            .find(|row| row.get("id") == Some(&ScalarValue::Int(*id)))
            .unwrap_or_else(|| panic!("falta la fila id = {id}"));
        assert_eq!(
            row.get("data"),
            Some(&ScalarValue::Text(data.clone())),
            "la fila {id} debe conservar su contenido"
        );
    }

    // No-colisión: ninguna página de tabla (heap/índice) es de overflow ni reservada.
    let overflow = catalog_overflow_pages(&mut database);
    assert!(
        !overflow.is_empty(),
        "el catálogo debe tener páginas de overflow"
    );
    let catalog = database.catalog().expect("catálogo");
    assert_eq!(catalog.table_names().len(), TABLES_TO_OVERFLOW + 1);
    for name in catalog.table_names() {
        let table = catalog.get(&name).expect("tabla del catálogo");
        for page in &table.pages {
            assert!(
                !overflow.contains(page),
                "la página {page} de la tabla '{name}' colisiona con una página de overflow"
            );
            assert!(
                *page >= 16,
                "la página {page} de la tabla '{name}' invade la región reservada 0..16"
            );
        }
    }
    assert_wide_table_intact(&mut database, &wide_table_name(TABLES_TO_OVERFLOW - 1));
}

/// AC-0086-03 — un catálogo legacy (≤ 16 páginas) carga sin migración.
///
/// Un catálogo pequeño se persiste en el formato legacy v1 (contiguo en la
/// región reservada) y al reabrir se carga sin error; la página 0 conserva la
/// versión `1`, por lo que el formato es intercambiable con el existente.
#[test]
// @spec AC-0086-03
fn test_ac_0086_03_legacy_catalog_within_reserved_pages_loads() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac86_03.db");
    {
        let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
        database
            .create_table(
                "small",
                vec![
                    ColumnDef {
                        name: "a".to_string(),
                        col_type: ColumnType::Int,
                    },
                    ColumnDef {
                        name: "b".to_string(),
                        col_type: ColumnType::Text,
                    },
                ],
            )
            .expect("create_table small");
        database
            .insert(
                "small",
                ScalarMap::from([
                    ("a".to_string(), ScalarValue::Int(1)),
                    ("b".to_string(), ScalarValue::Text("uno".to_string())),
                ]),
            )
            .expect("insert small");
        assert_eq!(
            catalog_version(&mut database),
            CATALOG_VERSION_RESERVED,
            "un catálogo pequeño debe seguir en formato legacy v1"
        );
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("reapertura");
    let names = database.tables().expect("tablas");
    assert_eq!(names, vec!["small".to_string()]);
    assert_eq!(catalog_version(&mut database), CATALOG_VERSION_RESERVED);
    let rows = database.execute("SELECT * FROM small").expect("select");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("b"),
        Some(&ScalarValue::Text("uno".to_string()))
    );
}
