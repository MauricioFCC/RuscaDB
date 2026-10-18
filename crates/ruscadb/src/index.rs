//! Índice secundario persistente de una columna (SPEC-0012, FR-0012-03).
//!
//! Cada página del índice guarda un array ordenado de entradas
//! `(key_bytes, localizador)` con formato
//! `[count u16 LE | (key_len u16 LE, key_bytes, page_id u64 LE, slot u32 LE)*]`.
//! La clave canónica preserva el orden (tag + big-endian / escape de texto) y
//! la búsqueda `Eq` usa búsqueda binaria. Tras cada inserción se reempaquetan
//! las páginas (válido para el volumen de la primera ruta vertical). Los
//! valores `NULL` no se indexan (en SQL `NULL` nunca iguala) y la búsqueda de
//! `NULL` devuelve vacío.

use ruscadb_core::{RuscaError, ScalarValue};
use ruscadb_storage::{PAGE_SIZE, Page, PageId};

use crate::catalog::{Catalog, ColumnType, TableDef};
use crate::database::Database;
use crate::heap::{RowLocator, heap_scan};

/// Etiqueta de clave booleana.
const TAG_BOOL: u8 = 0x01;
/// Etiqueta de clave entera.
const TAG_INT: u8 = 0x02;
/// Etiqueta de clave flotante.
const TAG_FLOAT: u8 = 0x03;
/// Etiqueta de clave de texto.
const TAG_TEXT: u8 = 0x04;

/// Bit de signo para ordenar enteros/flotantes con big-endian.
const SIGN_FLIP: u64 = 0x8000_0000_0000_0000;

/// Entrada del índice: clave canónica + localizador de la fila.
type IndexEntry = (Vec<u8>, RowLocator);

/// Codifica un valor escalar a su clave canónica que preserva el orden.
///
/// Args:
///     column_type: Tipo declarado de la columna indexada.
///     value: Valor a codificar (se admite coerción `Int↔Float`).
///
/// Returns:
///     Bytes comparables lexicográficamente en el orden de la columna.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si el valor no pertenece a la columna.
pub(crate) fn canonical_key(
    column_type: ColumnType,
    value: &ScalarValue,
) -> Result<Vec<u8>, RuscaError> {
    match (column_type, value) {
        (ColumnType::Bool, ScalarValue::Bool(flag)) => Ok(vec![TAG_BOOL, u8::from(*flag)]),
        (ColumnType::Int, ScalarValue::Int(number)) => Ok(int_key(*number)),
        (ColumnType::Int, ScalarValue::Float(number)) => int_from_float(*number),
        (ColumnType::Float, ScalarValue::Float(number)) => float_key(*number),
        (ColumnType::Float, ScalarValue::Int(number)) => float_key(*number as f64),
        (ColumnType::Text, ScalarValue::Text(text)) => Ok(text_key(text)),
        (_, ScalarValue::Null) => Err(type_mismatch("NULL no es indexable ni comparable")),
        _ => Err(type_mismatch(&format!(
            "el valor {value:?} no pertenece a una columna de tipo {column_type:?}"
        ))),
    }
}

/// Mantiene el índice de una columna con una entrada nueva y reempaqueta sus
/// páginas.
///
/// Los valores `NULL` se omiten (nunca igualan en una búsqueda `Eq`). A
/// diferencia de [`index_remove`], que falte el índice en esa columna es un
/// error: un caller que indexe una columna sin índice es un bug, no un no-op.
///
/// Args:
///     database: Base abierta.
///     catalog: Catálogo (asignador *bump* + definición de la tabla).
///     table_name: Tabla dueña del índice.
///     column: Columna del índice (SPEC-0104: una por cada índice de la tabla).
///     value: Valor indexado de la fila.
///     locator: Localizador de la fila.
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si la tabla no tiene índice en `column`;
///     [`RuscaError::TypeMismatch`] si el valor no pertenece a la columna.
pub(crate) fn index_insert(
    database: &mut Database,
    catalog: &mut Catalog,
    table_name: &str,
    column: &str,
    value: &ScalarValue,
    locator: RowLocator,
) -> Result<(), RuscaError> {
    if *value == ScalarValue::Null {
        return Ok(());
    }
    let (column_type, pages) = indexed_column(catalog.get(table_name)?, column)?;
    let key = canonical_key(column_type, value)?;
    let mut entries = load_entries(database, &pages)?;
    entries.push((key, locator));
    store_entries(database, catalog, table_name, column, entries)
}

/// Retira del índice de una columna la entrada de una fila borrada y
/// reempaqueta sus páginas.
///
/// No es un error que la tabla no tenga índice sobre esa columna ni que el
/// valor sea `NULL`: en ambos casos no hay entrada que retirar (a diferencia
/// de [`index_insert`], que exige el índice para escribir).
///
/// Args:
///     database: Base abierta.
///     catalog: Catálogo (asignador *bump* + definición de la tabla).
///     table_name: Tabla dueña del índice.
///     column: Columna del índice.
///     value: Valor indexado de la fila borrada.
///     locator: Localizador de la fila borrada.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si el valor no pertenece a la columna.
pub(crate) fn index_remove(
    database: &mut Database,
    catalog: &mut Catalog,
    table_name: &str,
    column: &str,
    value: &ScalarValue,
    locator: RowLocator,
) -> Result<(), RuscaError> {
    if *value == ScalarValue::Null {
        return Ok(());
    }
    // Sin índice en esa columna no hay entrada que retirar (SPEC-0104).
    if catalog.get(table_name)?.index_for(column).is_none() {
        return Ok(());
    }
    let (column_type, pages) = indexed_column(catalog.get(table_name)?, column)?;
    let key = canonical_key(column_type, value)?;
    let mut entries = load_entries(database, &pages)?;
    entries.retain(|(candidate, at)| !(candidate == &key && *at == locator));
    store_entries(database, catalog, table_name, column, entries)
}

/// Busca los localizadores con clave igual (`Eq` por búsqueda binaria).
///
/// Args:
///     database: Base abierta.
///     table: Tabla dueña del índice.
///     column: Columna del índice (SPEC-0104: puede haber varios índices).
///     value: Literal de igualdad (se admite coerción `Int↔Float`).
///
/// Returns:
///     Localizadores coincidentes (vacío si no hay o si el literal es `NULL`).
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si la tabla no tiene índice en esa columna;
///     [`RuscaError::TypeMismatch`] si el literal no pertenece a la columna.
pub(crate) fn index_lookup_eq(
    database: &mut Database,
    table: &TableDef,
    column: &str,
    value: &ScalarValue,
) -> Result<Vec<RowLocator>, RuscaError> {
    if *value == ScalarValue::Null {
        return Ok(Vec::new());
    }
    let (column_type, pages) = indexed_column(table, column)?;
    let key = canonical_key(column_type, value)?;
    let entries = load_entries(database, &pages)?;
    let lower = entries.partition_point(|(candidate, _)| *candidate < key);
    let upper = entries.partition_point(|(candidate, _)| *candidate <= key);
    Ok(entries[lower..upper]
        .iter()
        .map(|(_, locator)| *locator)
        .collect())
}

/// Reconstruye el índice de una columna desde las filas del heap.
///
/// Args:
///     database: Base abierta.
///     catalog: Catálogo (asignador *bump* + definición de la tabla).
///     table_name: Tabla dueña del índice.
///     column: Columna a indexar (debe tener índice en el catálogo).
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si la tabla no tiene índice en esa columna;
///     [`RuscaError::ColumnNotFound`] si una fila no trae la columna indexada;
///     [`RuscaError::TypeMismatch`] si un valor no pertenece a su columna.
pub(crate) fn index_build(
    database: &mut Database,
    catalog: &mut Catalog,
    table_name: &str,
    column: &str,
) -> Result<(), RuscaError> {
    let snapshot = catalog.get(table_name)?.clone();
    let (column_type, _) = indexed_column(&snapshot, column)?;
    let rows = heap_scan(database, &snapshot)?;
    let mut entries = Vec::with_capacity(rows.len());
    for (locator, record) in &rows {
        let value = record
            .scalars
            .get(column)
            .ok_or_else(|| RuscaError::ColumnNotFound {
                column: column.to_string(),
            })?;
        if *value == ScalarValue::Null {
            continue;
        }
        entries.push((canonical_key(column_type, value)?, *locator));
    }
    store_entries(database, catalog, table_name, column, entries)
}

/// Tipo y páginas del índice de una columna de la tabla.
///
/// Args:
///     table: Definición de la tabla.
///     column: Columna indexada.
///
/// Returns:
///     El tipo declarado de la columna y las páginas actuales de su índice.
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si la tabla no tiene índice en esa columna.
fn indexed_column(table: &TableDef, column: &str) -> Result<(ColumnType, Vec<u64>), RuscaError> {
    let Some(index) = table.index_for(column) else {
        return Err(RuscaError::InvalidConfig(format!(
            "la tabla '{}' no tiene índice en la columna '{column}' \
             (créalo con create_index)",
            table.name
        )));
    };
    Ok((table.column_type(column)?, index.pages.clone()))
}

/// Clave de un entero (inversión del signo + big-endian).
fn int_key(number: i64) -> Vec<u8> {
    let mut key = Vec::with_capacity(9);
    key.push(TAG_INT);
    key.extend_from_slice(&((number as u64 ^ SIGN_FLIP).to_be_bytes()));
    key
}

/// Clave de un flotante entero exacto hacia una columna `Int`.
fn int_from_float(number: f64) -> Result<Vec<u8>, RuscaError> {
    if number.fract() != 0.0 || !(i64::MIN as f64..=i64::MAX as f64).contains(&number) {
        return Err(type_mismatch(&format!(
            "el literal {number} no es un entero exacto de 64 bits"
        )));
    }
    Ok(int_key(number as i64))
}

/// Clave de un flotante (transformación que preserva el orden total).
fn float_key(number: f64) -> Result<Vec<u8>, RuscaError> {
    if number.is_nan() {
        return Err(type_mismatch("NaN no es ordenable ni indexable"));
    }
    let bits = if number == 0.0 {
        0u64
    } else {
        number.to_bits()
    };
    let ordered = if bits & SIGN_FLIP != 0 {
        !bits
    } else {
        bits ^ SIGN_FLIP
    };
    let mut key = Vec::with_capacity(9);
    key.push(TAG_FLOAT);
    key.extend_from_slice(&ordered.to_be_bytes());
    Ok(key)
}

/// Clave de texto con escape del `0x00` y terminador (preserva el orden).
fn text_key(text: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(text.len() + 3);
    key.push(TAG_TEXT);
    for byte in text.bytes() {
        if byte == 0x00 {
            key.extend_from_slice(&[0x00, 0xFF]);
        } else {
            key.push(byte);
        }
    }
    key.extend_from_slice(&[0x00, 0x00]);
    key
}

/// Construye un `TypeMismatch` con contexto de índice.
fn type_mismatch(message: &str) -> RuscaError {
    RuscaError::TypeMismatch {
        message: format!("índice: {message}"),
    }
}

/// Carga todas las entradas de un índice en memoria (ya ordenadas en disco).
///
/// Args:
///     database: Base abierta.
///     pages: Páginas del índice de la columna.
///
/// Returns:
///     Las entradas `(clave canónica, localizador)` en orden de página.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si una página está truncada;
///     [`RuscaError::PageOutOfRange`] si una página no existe.
fn load_entries(database: &mut Database, pages: &[u64]) -> Result<Vec<IndexEntry>, RuscaError> {
    let mut entries = Vec::new();
    for raw in pages {
        let page = database.read_page(PageId(*raw))?;
        entries.extend(parse_index_page(&page, *raw)?);
    }
    Ok(entries)
}

/// Analiza una página del índice validando límites.
fn parse_index_page(page: &Page, raw: u64) -> Result<Vec<IndexEntry>, RuscaError> {
    let data = page.data();
    let count = u16::from_le_bytes([data[0], data[1]]) as usize;
    let mut position = 2;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let key_length = read_entry_len(data, position, raw)? as usize;
        position += 2;
        let key = read_entry_key(data, position, key_length, raw)?;
        position += key_length;
        let locator = read_entry_locator(data, position, raw)?;
        position += 12;
        entries.push((key, locator));
    }
    Ok(entries)
}

/// Lee la longitud de clave de una entrada.
fn read_entry_len(data: &[u8], at: usize, raw: u64) -> Result<u16, RuscaError> {
    let slice = data.get(at..at + 2).ok_or_else(|| corrupt_index(raw))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

/// Lee los bytes de clave de una entrada.
fn read_entry_key(data: &[u8], at: usize, length: usize, raw: u64) -> Result<Vec<u8>, RuscaError> {
    data.get(at..at + length)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| corrupt_index(raw))
}

/// Lee el localizador `(PageId, slot)` de una entrada.
fn read_entry_locator(data: &[u8], at: usize, raw: u64) -> Result<RowLocator, RuscaError> {
    let slice = data.get(at..at + 12).ok_or_else(|| corrupt_index(raw))?;
    let mut page_raw = [0u8; 8];
    page_raw.copy_from_slice(&slice[0..8]);
    let mut slot_raw = [0u8; 4];
    slot_raw.copy_from_slice(&slice[8..12]);
    Ok((
        PageId(u64::from_le_bytes(page_raw)),
        u32::from_le_bytes(slot_raw),
    ))
}

/// Construye un error de índice corrupto.
fn corrupt_index(raw: u64) -> RuscaError {
    RuscaError::CorruptManifest(format!("índice corrupto: página {raw} truncada"))
}

/// Ordena y escribe las entradas en las páginas del índice de una columna.
///
/// Args:
///     database: Base abierta.
///     catalog: Catálogo (asignador *bump* + páginas del índice).
///     table_name: Tabla dueña del índice.
///     column: Columna cuyo índice se reescribe.
///     entries: Entradas a empaquetar (se ordenan por clave canónica).
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si una clave no cabe en una página;
///     [`RuscaError::Io`] si falla la escritura de una página.
fn store_entries(
    database: &mut Database,
    catalog: &mut Catalog,
    table_name: &str,
    column: &str,
    mut entries: Vec<IndexEntry>,
) -> Result<(), RuscaError> {
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let payloads = pack_entries(&entries)?;
    let mut page_ids = catalog.get(table_name)?.index_pages(column);
    while page_ids.len() < payloads.len() {
        // Reutiliza páginas liberadas antes de crecer (SPEC-0085).
        page_ids.push(database.allocate_page(catalog).0);
    }
    page_ids.truncate(payloads.len());
    for (raw, payload) in page_ids.iter().zip(payloads.iter()) {
        let mut page = Page::new(PageId(*raw));
        page.data_mut()[..payload.len()].copy_from_slice(payload);
        database.write_page(&page)?;
    }
    catalog
        .get_mut(table_name)?
        .set_index_pages(column, page_ids);
    Ok(())
}

/// Reparte las entradas ordenadas en cargas de página (puro, testeable).
fn pack_entries(entries: &[IndexEntry]) -> Result<Vec<Vec<u8>>, RuscaError> {
    let mut payloads: Vec<Vec<u8>> = Vec::new();
    let mut current = vec![0u8, 0u8];
    let mut count = 0u16;
    for (key, (page, slot)) in entries {
        let needed = 2 + key.len() + 8 + 4;
        if needed > PAGE_SIZE - 2 {
            return Err(RuscaError::InvalidConfig(format!(
                "la clave de {} bytes supera la página de 4 KiB (valor demasiado largo para indexar)",
                key.len()
            )));
        }
        if current.len() + needed > PAGE_SIZE {
            seal_payload(&mut current, &mut payloads, count);
            count = 0;
        }
        push_entry(&mut current, key, page.0, *slot);
        count += 1;
    }
    seal_payload(&mut current, &mut payloads, count);
    Ok(payloads)
}

/// Sella la página en curso (fija el contador) y empieza una vacía.
fn seal_payload(current: &mut Vec<u8>, payloads: &mut Vec<Vec<u8>>, count: u16) {
    if count == 0 {
        return;
    }
    current[0..2].copy_from_slice(&count.to_le_bytes());
    let finished = std::mem::replace(current, vec![0u8, 0u8]);
    payloads.push(finished);
}

/// Añade una entrada al buffer de página (fija el contador al cerrar).
fn push_entry(current: &mut Vec<u8>, key: &[u8], page: u64, slot: u32) {
    current.extend_from_slice(&(key.len() as u16).to_le_bytes());
    current.extend_from_slice(key);
    current.extend_from_slice(&page.to_le_bytes());
    current.extend_from_slice(&slot.to_le_bytes());
}
