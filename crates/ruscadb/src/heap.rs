//! Heap de filas en páginas ranuradas (SPEC-0012, FR-0012-02).
//!
//! Formato de página: `[slot_count u16 LE | slots (offset u16, len u16)* |
//! filas]`; las filas (`postcard(Record)` con solo `scalars` poblados) crecen
//! desde el final de la página. El localizador de una fila es `(PageId,
//! slot)`. Una fila mayor que la página se rechaza con
//! [`RuscaError::InvalidConfig`] (límite documentado: 4 KiB por fila,
//! cabida máxima de `4090` bytes serializados).

use ruscadb_core::{Record, RuscaError};
use ruscadb_storage::{PAGE_SIZE, Page, PageId};

use crate::catalog::Catalog;
use crate::database::Database;

/// Localizador físico de una fila: página + índice de slot.
pub type RowLocator = (PageId, u32);

/// Tamaño del contador de slots en bytes.
const COUNT_SIZE: usize = 2;

/// Tamaño de un slot `(offset, len)` en bytes.
const SLOT_SIZE: usize = 4;

/// Cabida máxima de una fila serializada en una página de 4 KiB.
const MAX_ROW_BYTES: usize = PAGE_SIZE - COUNT_SIZE - SLOT_SIZE;

/// Inserta un registro en el heap de la tabla.
///
/// Busca sitio en la última página con hueco y, si no hay, reserva una
/// página nueva del asignador *bump*. Actualiza `row_count`.
///
/// Args:
///     database: Base abierta.
///     catalog: Catálogo (asignador *bump* + definición de la tabla).
///     table_name: Tabla destino.
///     record: Registro a almacenar.
///
/// Returns:
///     El localizador `(PageId, slot)` de la fila.
///
/// Errors:
///     [`RuscaError::TableNotFound`] si la tabla no existe;
///     [`RuscaError::InvalidConfig`] si la fila no cabe en 4 KiB.
pub fn heap_insert(
    database: &mut Database,
    catalog: &mut Catalog,
    table_name: &str,
    record: &Record,
) -> Result<RowLocator, RuscaError> {
    let bytes = serialize_row(record)?;
    let existing: Vec<u64> = catalog.get(table_name)?.pages.clone();
    for raw in existing.iter().rev() {
        let page = database.read_page(PageId(*raw))?;
        if let Some(slot) = try_insert_into(&page, &bytes)? {
            let table = catalog.get_mut(table_name)?;
            finish_insert(database, table, &page, *raw, &bytes, slot)?;
            return Ok((PageId(*raw), slot));
        }
    }
    let fresh = catalog.alloc_page();
    let blank = blank_slotted_page(fresh);
    let table = catalog.get_mut(table_name)?;
    finish_insert(database, table, &blank, fresh.0, &bytes, 0)?;
    Ok((fresh, 0))
}

/// Lee una fila por su localizador.
///
/// Args:
///     database: Base abierta.
///     locator: Localizador `(PageId, slot)`.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el slot o los bytes son inválidos.
pub fn heap_read(database: &mut Database, locator: RowLocator) -> Result<Record, RuscaError> {
    let page = database.read_page(locator.0)?;
    let slots = parse_slots(page.data())?;
    let (offset, length) = slots.get(locator.1 as usize).ok_or_else(|| {
        corrupt_heap(&format!(
            "slot {} inexistente en la página {}",
            locator.1, locator.0.0
        ))
    })?;
    decode_row(page.data(), *offset, *length)
}

/// Reescribe una fila en su localizador, recalculando los offsets de la página.
///
/// Conserva el localizador `(PageId, slot)` (el número y el orden de los slots
/// no cambian): reempaqueta todas las filas desde el final de la página, de modo
/// que la fila actualizada puede cambiar de longitud sin solapar a las demás.
///
/// Args:
///     database: Base abierta.
///     locator: Localizador `(PageId, slot)` de la fila a reescribir.
///     record: Versión actualizada del registro.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el slot no existe o el heap es inválido;
///     [`RuscaError::InvalidConfig`] si la fila serializada no cabe en 4 KiB.
pub fn heap_update(
    database: &mut Database,
    locator: RowLocator,
    record: &Record,
) -> Result<(), RuscaError> {
    let bytes = serialize_row(record)?;
    let page = database.read_page(locator.0)?;
    let slots = parse_slots(page.data())?;
    let target = locator.1 as usize;
    if target >= slots.len() {
        return Err(corrupt_heap(&format!(
            "slot {} inexistente en la página {}",
            locator.1, locator.0.0
        )));
    }
    let repacked = repack_page(locator.0, page.data(), &slots, target, &bytes)?;
    database.write_page(&repacked)
}

/// Serializa una fila validando la cabida en una página de 4 KiB.
///
/// Args:
///     record: Registro a serializar.
///
/// Returns:
///     Los bytes `postcard(Record)`.
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si la serialización falla o no cabe.
fn serialize_row(record: &Record) -> Result<Vec<u8>, RuscaError> {
    let bytes = postcard::to_allocvec(record).map_err(|error| {
        RuscaError::InvalidConfig(format!("la fila no se pudo serializar: {error}"))
    })?;
    if bytes.len() > MAX_ROW_BYTES {
        return Err(RuscaError::InvalidConfig(format!(
            "la fila serializada ocupa {} bytes y supera el límite de {MAX_ROW_BYTES} por página de 4 KiB (reduce columnas o el documento)",
            bytes.len()
        )));
    }
    Ok(bytes)
}

/// Reempaqueta la página sustituyendo el slot `target` por `replacement`.
///
/// Args:
///     id: Página reescrita.
///     data: Bytes actuales de la página.
///     slots: Directorio de slots validado.
///     target: Índice del slot a sustituir.
///     replacement: Bytes nuevos de la fila.
///
/// Returns:
///     La página reempaquetada con los mismos slots en el mismo orden.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el resultado no cabe en la página.
fn repack_page(
    id: PageId,
    data: &[u8],
    slots: &[(u16, u16)],
    target: usize,
    replacement: &[u8],
) -> Result<Page, RuscaError> {
    repack_page_fits(slots, target, replacement)?;
    let mut page = Page::new(id);
    let mut cursor = PAGE_SIZE;
    let out = page.data_mut();
    for (index, (offset, length)) in slots.iter().enumerate() {
        let row: &[u8] = if index == target {
            replacement
        } else {
            let (start, end) = (*offset as usize, *offset as usize + *length as usize);
            data.get(start..end)
                .ok_or_else(|| corrupt_heap("un slot apunta fuera de la página"))?
        };
        cursor -= row.len();
        out[cursor..cursor + row.len()].copy_from_slice(row);
        let at = COUNT_SIZE + index * SLOT_SIZE;
        out[at..at + COUNT_SIZE].copy_from_slice(&(cursor as u16).to_le_bytes());
        out[at + COUNT_SIZE..at + SLOT_SIZE].copy_from_slice(&(row.len() as u16).to_le_bytes());
    }
    set_slot_count(out, slots.len() as u16);
    Ok(page)
}

/// Comprueba que el reempaquetado (directorio + filas) cabe en la página.
///
/// Args:
///     slots: Directorio de slots validado.
///     target: Índice del slot sustituido.
///     replacement: Bytes nuevos de la fila.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el resultado excede los 4 KiB.
fn repack_page_fits(
    slots: &[(u16, u16)],
    target: usize,
    replacement: &[u8],
) -> Result<(), RuscaError> {
    let directory_end = COUNT_SIZE + slots.len() * SLOT_SIZE;
    let rows_total: usize = slots
        .iter()
        .enumerate()
        .map(|(index, (_, length))| {
            if index == target {
                replacement.len()
            } else {
                *length as usize
            }
        })
        .sum();
    if directory_end + rows_total > PAGE_SIZE {
        return Err(corrupt_heap(
            "la fila actualizada no cabe en la página de 4 KiB",
        ));
    }
    Ok(())
}

/// Recorre todas las filas del heap en orden de inserción.
///
/// Args:
///     database: Base abierta.
///     table: Definición de la tabla.
///
/// Returns:
///     Pares `(localizador, registro)` en orden de inserción.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si una página del heap es inválida.
pub fn heap_scan(
    database: &mut Database,
    table: &crate::catalog::TableDef,
) -> Result<Vec<(RowLocator, Record)>, RuscaError> {
    let mut rows = Vec::with_capacity(table.row_count as usize);
    for raw in &table.pages {
        collect_page_rows(database, PageId(*raw), &mut rows)?;
    }
    Ok(rows)
}

/// Inicializa una página ranurada vacía.
///
/// Args:
///     id: Identificador de la página.
///
/// Returns:
///     Página con contador de slots a cero.
pub fn blank_slotted_page(id: PageId) -> Page {
    let mut page = Page::new(id);
    page.data_mut()[0..COUNT_SIZE].copy_from_slice(&0u16.to_le_bytes());
    page
}

/// Lee las filas de una página y las añade al acumulador.
fn collect_page_rows(
    database: &mut Database,
    id: PageId,
    rows: &mut Vec<(RowLocator, Record)>,
) -> Result<(), RuscaError> {
    let page = database.read_page(id)?;
    let slots = parse_slots(page.data())?;
    for (slot, (offset, length)) in slots.iter().enumerate() {
        rows.push((
            (id, slot as u32),
            decode_row(page.data(), *offset, *length)?,
        ));
    }
    Ok(())
}

/// Intenta ubicar la fila en la página; devuelve el slot si cabe.
fn try_insert_into(page: &Page, bytes: &[u8]) -> Result<Option<u32>, RuscaError> {
    let slots = parse_slots(page.data())?;
    let used = COUNT_SIZE + slots.len() * SLOT_SIZE;
    let lowest = slots
        .iter()
        .map(|(offset, _)| *offset)
        .min()
        .unwrap_or(PAGE_SIZE as u16);
    let free = (lowest as usize).saturating_sub(used);
    if free < SLOT_SIZE + bytes.len() {
        return Ok(None);
    }
    Ok(Some(slots.len() as u32))
}

/// Escribe la fila en la página (slot + bytes) y actualiza la tabla.
fn finish_insert(
    database: &mut Database,
    table: &mut crate::catalog::TableDef,
    template: &Page,
    raw: u64,
    bytes: &[u8],
    slot: u32,
) -> Result<(), RuscaError> {
    let slots = parse_slots(template.data())?;
    let lowest = slots
        .iter()
        .map(|(offset, _)| *offset)
        .min()
        .unwrap_or(PAGE_SIZE as u16);
    let offset = lowest as usize - bytes.len();
    let mut page = Page::new(PageId(raw));
    page.data_mut().copy_from_slice(template.data());
    let data = page.data_mut();
    let at = COUNT_SIZE + slot as usize * SLOT_SIZE;
    data[at..at + COUNT_SIZE].copy_from_slice(&(offset as u16).to_le_bytes());
    data[at + COUNT_SIZE..at + SLOT_SIZE].copy_from_slice(&(bytes.len() as u16).to_le_bytes());
    data[offset..offset + bytes.len()].copy_from_slice(bytes);
    set_slot_count(data, slot as u16 + 1);
    database.write_page(&page)?;
    if !table.pages.contains(&raw) {
        if table.pages.is_empty() {
            table.heap_start = PageId(raw);
        }
        table.pages.push(raw);
    }
    table.row_count += 1;
    Ok(())
}

/// Fija el contador de slots de una página.
fn set_slot_count(data: &mut [u8], count: u16) {
    data[0..COUNT_SIZE].copy_from_slice(&count.to_le_bytes());
}

/// Analiza el directorio de slots validando límites.
fn parse_slots(data: &[u8]) -> Result<Vec<(u16, u16)>, RuscaError> {
    let count = read_count(data) as usize;
    let end = COUNT_SIZE + count * SLOT_SIZE;
    if end > PAGE_SIZE {
        return Err(corrupt_heap("el directorio de slots excede la página"));
    }
    let mut slots = Vec::with_capacity(count);
    for index in 0..count {
        let at = COUNT_SIZE + index * SLOT_SIZE;
        let offset = u16::from_le_bytes([data[at], data[at + 1]]);
        let length = u16::from_le_bytes([data[at + 2], data[at + 3]]);
        validate_slot(data, end, offset, length)?;
        slots.push((offset, length));
    }
    check_slot_overlap(&slots)?;
    Ok(slots)
}

/// Lee el contador de slots.
fn read_count(data: &[u8]) -> u16 {
    u16::from_le_bytes([data[0], data[1]])
}

/// Valida que un slot apunte dentro de la zona de filas.
fn validate_slot(
    data: &[u8],
    directory_end: usize,
    offset: u16,
    length: u16,
) -> Result<(), RuscaError> {
    let (start, end) = (offset as usize, offset as usize + length as usize);
    if start < directory_end || end > data.len() {
        return Err(corrupt_heap("un slot apunta fuera de la zona de filas"));
    }
    Ok(())
}

/// Comprueba que las filas no se solapen entre sí.
fn check_slot_overlap(slots: &[(u16, u16)]) -> Result<(), RuscaError> {
    let mut spans: Vec<(usize, usize)> = slots
        .iter()
        .map(|(offset, length)| (*offset as usize, *offset as usize + *length as usize))
        .collect();
    spans.sort();
    for pair in spans.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(corrupt_heap("dos filas del heap se solapan"));
        }
    }
    Ok(())
}

/// Decodifica una fila `postcard(Record)`.
fn decode_row(data: &[u8], offset: u16, length: u16) -> Result<Record, RuscaError> {
    let (start, end) = (offset as usize, offset as usize + length as usize);
    let slice = data
        .get(start..end)
        .ok_or_else(|| corrupt_heap("fila fuera de la página"))?;
    postcard::from_bytes(slice)
        .map_err(|error| corrupt_heap(&format!("fila con bytes inválidos: {error}")))
}

/// Construye un error de heap corrupto.
fn corrupt_heap(message: &str) -> RuscaError {
    RuscaError::CorruptManifest(format!("heap corrupto: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruscadb_core::{EdgeSet, RecordId, RecordMeta, ScalarMap, ScalarValue};

    /// Construye un registro cuyo payload `Bytes` tiene `size` bytes.
    fn record_with_payload(size: usize) -> Record {
        let mut scalars = ScalarMap::new();
        scalars.insert("data".to_string(), ScalarValue::Bytes(vec![0u8; size]));
        Record {
            id: RecordId::new(),
            scalars,
            doc: None,
            edges: EdgeSet::default(),
            vector: None,
            blob: None,
            meta: RecordMeta::default(),
        }
    }

    /// Encuentra un registro que serializa exactamente a `target` bytes.
    fn record_of_serialized_len(target: usize) -> Record {
        for size in 0..=target + 32 {
            let record = record_with_payload(size);
            if postcard::to_allocvec(&record).map(|bytes| bytes.len()) == Ok(target) {
                return record;
            }
        }
        panic!("no se encontró un registro de {target} bytes");
    }

    /// SPEC-0022 — `serialize_row` acepta exactamente `MAX_ROW_BYTES` y rechaza
    /// un byte más (BVA del límite de página).
    #[test]
    fn test_ac_0022_serialize_row_size_boundary() {
        let exact = record_of_serialized_len(MAX_ROW_BYTES);
        assert!(
            serialize_row(&exact).is_ok(),
            "una fila de exactamente {MAX_ROW_BYTES} bytes cabe"
        );
        let over = record_of_serialized_len(MAX_ROW_BYTES + 1);
        assert!(
            serialize_row(&over).is_err(),
            "una fila de {} bytes debe rechazarse",
            MAX_ROW_BYTES + 1
        );
    }

    /// Página ranurada sintética con dos filas en `(offset, bytes)`.
    fn page_with(rows: &[(u16, &[u8])]) -> (Vec<u8>, Vec<(u16, u16)>) {
        let mut data = vec![0u8; PAGE_SIZE];
        let mut slots = Vec::new();
        for (offset, bytes) in rows {
            data[*offset as usize..*offset as usize + bytes.len()].copy_from_slice(bytes);
            slots.push((*offset, bytes.len() as u16));
        }
        (data, slots)
    }

    /// SPEC-0022 — `repack_page` conserva el orden de slots y reescribe solo el
    /// slot objetivo, recalculando offsets desde el final de la página.
    #[test]
    fn test_ac_0022_repack_page_preserves_order_and_content() {
        let (data, slots) = page_with(&[(100, &[10, 11, 12, 13]), (200, &[20, 21, 22, 23])]);
        let replacement = [1u8, 2, 3, 4, 5];
        let page = repack_page(PageId(7), &data, &slots, 0, &replacement).expect("repack");

        assert_eq!(page.id(), PageId(7));
        let parsed = parse_slots(page.data()).expect("slots válidos");
        assert_eq!(parsed.len(), 2, "el número de slots no cambia");
        assert!(
            parsed[0].0 > parsed[1].0,
            "las filas crecen desde el final de la página"
        );
        let read = |slot: (u16, u16)| -> Vec<u8> {
            page.data()[slot.0 as usize..slot.0 as usize + slot.1 as usize].to_vec()
        };
        assert_eq!(
            read(parsed[0]),
            replacement.to_vec(),
            "slot objetivo sustituido"
        );
        assert_eq!(
            read(parsed[1]),
            vec![20, 21, 22, 23],
            "el otro slot se conserva"
        );
    }

    /// SPEC-0022 — `repack_page` acepta un reempaquetado que llena la página
    /// exactamente y rechaza uno que la desborda por un byte (BVA del límite).
    #[test]
    fn test_ac_0022_repack_page_size_boundary() {
        let other_offset = 2000u16;
        let other = vec![7u8; (PAGE_SIZE - COUNT_SIZE - 2 * SLOT_SIZE) - 2000];
        let (data, mut slots) = page_with(&[(other_offset, &other)]);
        slots.insert(0, (0, 0)); // slot objetivo (reemplazado por `replacement`)

        let fits = vec![9u8; 2000];
        assert!(
            repack_page(PageId(3), &data, &slots, 0, &fits).is_ok(),
            "un reempaquetado que llena la página cabe"
        );

        let overflows = vec![9u8; 2001];
        assert!(
            repack_page(PageId(3), &data, &slots, 0, &overflows).is_err(),
            "un reempaquetado que desborda la página falla"
        );
    }
}
