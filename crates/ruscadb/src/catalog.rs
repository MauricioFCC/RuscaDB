//! Catálogo persistente de tablas (SPEC-0012, FR-0012-01).
//!
//! El [`Catalog`] mapea nombres de tabla a su [`TableDef`] y actúa como
//! asignador *bump* de páginas (`next_page`, sin free-list). Se persiste a
//! partir de la **página 0**: superblock de 16 bytes
//! `[magic u64 LE | versión u32 LE | páginas de catálogo u32 LE]` seguido de
//! los bytes del catálogo (que pueden extenderse a `0..catalog_pages`).
//!
//! El códec es manual (enteros LE y cadenas con prefijo `u32 LE`) para no
//! añadir dependencias de serialización a la fachada. Las primeras
//! [`CATALOG_RESERVED_PAGES`] páginas están reservadas al catálogo: el heap
//! empieza después, por lo que el crecimiento del catálogo nunca colisiona
//! con filas ya asignadas (sin free-list, SPEC-0012 §Contexto).

use std::collections::BTreeMap;

use ruscadb_core::RuscaError;
use ruscadb_storage::{PAGE_SIZE, Page, PageId};

use crate::Database;

/// Magia del superblock (`"RUSCDB01"`): distingue la página 0 del catálogo.
pub const CATALOG_MAGIC: u64 = 0x5255_5343_4442_3031;

/// Versión del formato del catálogo persistido.
pub const CATALOG_VERSION: u32 = 1;

/// Tamaño del superblock en bytes (magic u64 + versión u32 + páginas u32).
const SUPERBLOCK_SIZE: usize = 16;

/// Páginas reservadas al catálogo (`0..16` = 64 KiB, cientos de tablas).
const CATALOG_RESERVED_PAGES: u64 = 16;

/// Tipo de dato de una columna relacional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnType {
    /// Booleano.
    Bool,
    /// Entero con signo de 64 bits.
    Int,
    /// Flotante de 64 bits.
    Float,
    /// Texto UTF-8.
    Text,
}

/// Definición de una columna (`nombre + tipo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnDef {
    /// Nombre de la columna.
    pub name: String,
    /// Tipo declarado.
    pub col_type: ColumnType,
}

/// Definición del índice secundario de una tabla (una columna, SPEC-0012 §FR-0012-03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexDef {
    /// Columna indexada.
    pub column: String,
    /// Páginas propias del índice (array ordenado repartido en páginas).
    pub pages: Vec<u64>,
}

/// Definición persistida de una tabla.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableDef {
    /// Nombre de la tabla.
    pub name: String,
    /// Esquema de columnas.
    pub columns: Vec<ColumnDef>,
    /// Primera página del heap (localizador inicial de las filas).
    pub heap_start: PageId,
    /// Páginas del heap en orden de inserción (el heap no es contiguo:
    /// comparte el asignador *bump* con el índice).
    pub pages: Vec<u64>,
    /// Número de filas vivas.
    pub row_count: u64,
    /// Índice secundario (una columna por tabla).
    pub index: Option<IndexDef>,
}

/// Catálogo de tablas + asignador *bump* de páginas libres.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    /// Tablas por nombre.
    tables: BTreeMap<String, TableDef>,
    /// Siguiente página libre (todo `id >= next_page` está sin asignar).
    next_page: u64,
}

impl Catalog {
    /// Crea un catálogo vacío (el heap empieza tras la región reservada).
    ///
    /// Returns:
    ///     Catálogo sin tablas y con `next_page = 16`.
    pub fn new() -> Self {
        Self {
            tables: BTreeMap::new(),
            next_page: CATALOG_RESERVED_PAGES,
        }
    }

    /// Indica si existe la tabla.
    ///
    /// Args:
    ///     name: Nombre de la tabla.
    pub fn contains(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }

    /// Nombres de las tablas del catálogo, en orden.
    pub fn table_names(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }

    /// Siguiente página libre del asignador *bump*.
    pub fn next_page(&self) -> u64 {
        self.next_page
    }

    /// Busca una tabla por nombre.
    ///
    /// Args:
    ///     name: Nombre de la tabla.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si no existe.
    pub fn get(&self, name: &str) -> Result<&TableDef, RuscaError> {
        self.tables
            .get(name)
            .ok_or_else(|| RuscaError::TableNotFound {
                table: name.to_string(),
            })
    }

    /// Busca una tabla por nombre (mutable).
    ///
    /// Args:
    ///     name: Nombre de la tabla.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si no existe.
    pub fn get_mut(&mut self, name: &str) -> Result<&mut TableDef, RuscaError> {
        self.tables
            .get_mut(name)
            .ok_or_else(|| RuscaError::TableNotFound {
                table: name.to_string(),
            })
    }

    /// Reserva la siguiente página libre (*bump allocator*).
    ///
    /// Returns:
    ///     El `PageId` reservado.
    pub fn alloc_page(&mut self) -> PageId {
        let id = PageId(self.next_page);
        self.next_page += 1;
        id
    }

    /// Carga el catálogo desde las páginas de la base.
    ///
    /// Si la página 0 no existe o no contiene la magia, devuelve un
    /// catálogo vacío (base nueva).
    ///
    /// Args:
    ///     database: Base abierta.
    pub(crate) fn load(database: &mut Database) -> Result<Self, RuscaError> {
        let first = match database.read_page(PageId(0)) {
            Ok(page) => page,
            Err(RuscaError::PageOutOfRange { .. }) => return Ok(Self::new()),
            Err(other) => return Err(other),
        };
        if read_magic(first.data()) != CATALOG_MAGIC {
            return Ok(Self::new());
        }
        let catalog_pages = read_catalog_pages(first.data()) as u64;
        if catalog_pages == 0 {
            return Err(corrupt("el superblock declara 0 páginas de catálogo"));
        }
        let mut payload = Vec::with_capacity(catalog_pages as usize * PAGE_SIZE);
        for raw in 0..catalog_pages {
            let page = database
                .read_page(PageId(raw))
                .map_err(|error| match error {
                    RuscaError::PageOutOfRange { .. } => corrupt(&format!(
                        "el catálogo declara {catalog_pages} páginas pero falta la {raw}"
                    )),
                    other => other,
                })?;
            let skip = if raw == 0 { SUPERBLOCK_SIZE } else { 0 };
            payload.extend_from_slice(&page.data()[skip..]);
        }
        decode_catalog(&payload)
    }

    /// Persiste el catálogo en `0..catalog_pages` y confirma (WAL-first).
    ///
    /// Incluye en el mismo commit las páginas sucias previas (heap/índice),
    /// por lo que `insert` logra atomicidad con una sola llamada.
    ///
    /// Args:
    ///     database: Base abierta.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el catálogo supera la región
    ///     reservada (límite documentado: 16 páginas = 64 KiB).
    pub(crate) fn save(&mut self, database: &mut Database) -> Result<(), RuscaError> {
        let bytes = encode_catalog(self);
        let pages = div_ceil(SUPERBLOCK_SIZE + bytes.len(), PAGE_SIZE) as u64;
        if pages > CATALOG_RESERVED_PAGES {
            return Err(RuscaError::InvalidConfig(format!(
                "el catálogo ocupa {} páginas y supera la región reservada de {CATALOG_RESERVED_PAGES} (reduce tablas/columnas)",
                pages
            )));
        }
        if self.next_page < CATALOG_RESERVED_PAGES {
            self.next_page = CATALOG_RESERVED_PAGES;
        }
        let total = pages as usize * PAGE_SIZE;
        let mut buffer = vec![0u8; total];
        buffer[0..8].copy_from_slice(&CATALOG_MAGIC.to_le_bytes());
        buffer[8..12].copy_from_slice(&CATALOG_VERSION.to_le_bytes());
        buffer[12..16].copy_from_slice(&(pages as u32).to_le_bytes());
        buffer[SUPERBLOCK_SIZE..SUPERBLOCK_SIZE + bytes.len()].copy_from_slice(&bytes);
        for (index, chunk) in buffer.chunks(PAGE_SIZE).enumerate() {
            let mut page = Page::new(PageId(index as u64));
            page.data_mut().copy_from_slice(chunk);
            database.write_page(&page)?;
        }
        database.commit()?;
        Ok(())
    }

    /// Registra una tabla nueva con su primera página de heap reservada.
    ///
    /// Args:
    ///     name: Nombre de la tabla.
    ///     columns: Esquema validado por el llamador.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la tabla ya existe.
    pub(crate) fn register_table(
        &mut self,
        name: &str,
        columns: Vec<ColumnDef>,
    ) -> Result<(), RuscaError> {
        if self.tables.contains_key(name) {
            return Err(RuscaError::InvalidConfig(format!(
                "la tabla '{name}' ya existe en el catálogo (elige otro nombre)"
            )));
        }
        let heap_start = self.alloc_page();
        self.tables.insert(
            name.to_string(),
            TableDef {
                name: name.to_string(),
                columns,
                heap_start,
                pages: vec![heap_start.0],
                row_count: 0,
                index: None,
            },
        );
        Ok(())
    }
}

impl TableDef {
    /// Tipo declarado de una columna.
    ///
    /// Args:
    ///     column: Nombre de la columna.
    ///
    /// Errors:
    ///     [`RuscaError::ColumnNotFound`] si no pertenece al esquema.
    pub fn column_type(&self, column: &str) -> Result<ColumnType, RuscaError> {
        self.columns
            .iter()
            .find(|definition| definition.name == column)
            .map(|definition| definition.col_type)
            .ok_or_else(|| RuscaError::ColumnNotFound {
                column: column.to_string(),
            })
    }

    /// Páginas del índice secundario (vacío si no hay índice).
    pub(crate) fn index_pages(&self) -> Vec<u64> {
        self.index
            .as_ref()
            .map(|index| index.pages.clone())
            .unwrap_or_default()
    }

    /// Sustituye las páginas del índice (no-op si no hay índice).
    ///
    /// Args:
    ///     pages: Nuevas páginas del índice.
    pub(crate) fn set_index_pages(&mut self, pages: Vec<u64>) {
        if let Some(index) = self.index.as_mut() {
            index.pages = pages;
        }
    }
}

/// Lee la magia del superblock de una página.
fn read_magic(data: &[u8]) -> u64 {
    let mut magic = [0u8; 8];
    magic.copy_from_slice(&data[0..8]);
    u64::from_le_bytes(magic)
}

/// Lee el nº de páginas del catálogo del superblock.
fn read_catalog_pages(data: &[u8]) -> u32 {
    let mut pages = [0u8; 4];
    pages.copy_from_slice(&data[12..16]);
    u32::from_le_bytes(pages)
}

/// División entera redondeando hacia arriba.
fn div_ceil(value: usize, divisor: usize) -> usize {
    value.div_ceil(divisor)
}

/// Construye un error de catálogo corrupto.
fn corrupt(message: &str) -> RuscaError {
    RuscaError::CorruptManifest(format!("catálogo corrupto: {message}"))
}

/// Añade un `u32` LE al buffer.
fn push_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

/// Añade un `u64` LE al buffer.
fn push_u64(buffer: &mut Vec<u8>, value: u64) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

/// Añade una cadena (prefijo `u32 LE` + bytes UTF-8).
fn push_str(buffer: &mut Vec<u8>, value: &str) {
    push_u32(buffer, value.len() as u32);
    buffer.extend_from_slice(value.as_bytes());
}

/// Etiqueta binaria de un tipo de columna.
fn column_tag(column_type: ColumnType) -> u8 {
    match column_type {
        ColumnType::Bool => 0,
        ColumnType::Int => 1,
        ColumnType::Float => 2,
        ColumnType::Text => 3,
    }
}

/// Serializa el catálogo a bytes.
fn encode_catalog(catalog: &Catalog) -> Vec<u8> {
    let mut buffer = Vec::new();
    push_u32(&mut buffer, catalog.tables.len() as u32);
    for (name, table) in &catalog.tables {
        push_str(&mut buffer, name);
        encode_table(&mut buffer, table);
    }
    push_u64(&mut buffer, catalog.next_page);
    buffer
}

/// Serializa una tabla al buffer.
fn encode_table(buffer: &mut Vec<u8>, table: &TableDef) {
    push_str(buffer, &table.name);
    push_u32(buffer, table.columns.len() as u32);
    for column in &table.columns {
        push_str(buffer, &column.name);
        buffer.push(column_tag(column.col_type));
    }
    push_u64(buffer, table.heap_start.0);
    push_u32(buffer, table.pages.len() as u32);
    for page in &table.pages {
        push_u64(buffer, *page);
    }
    push_u64(buffer, table.row_count);
    match &table.index {
        Some(index) => {
            buffer.push(1);
            push_str(buffer, &index.column);
            push_u32(buffer, index.pages.len() as u32);
            for page in &index.pages {
                push_u64(buffer, *page);
            }
        }
        None => buffer.push(0),
    }
}

/// Lector con comprobación de límites sobre los bytes del catálogo.
struct Decoder<'bytes> {
    /// Bytes de entrada.
    bytes: &'bytes [u8],
    /// Cursor de lectura.
    position: usize,
}

impl<'bytes> Decoder<'bytes> {
    /// Crea un lector sobre los bytes dados.
    fn new(bytes: &'bytes [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    /// Lee un `u8` o falla si no hay bytes.
    fn read_u8(&mut self) -> Result<u8, RuscaError> {
        let byte = self
            .bytes
            .get(self.position)
            .ok_or_else(|| corrupt("bytes truncados al leer u8"))?;
        self.position += 1;
        Ok(*byte)
    }

    /// Lee un `u32` LE o falla si no hay bytes.
    fn read_u32(&mut self) -> Result<u32, RuscaError> {
        let slice = self
            .bytes
            .get(self.position..self.position + 4)
            .ok_or_else(|| corrupt("bytes truncados al leer u32"))?;
        self.position += 4;
        Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    /// Lee un `u64` LE o falla si no hay bytes.
    fn read_u64(&mut self) -> Result<u64, RuscaError> {
        let slice = self
            .bytes
            .get(self.position..self.position + 8)
            .ok_or_else(|| corrupt("bytes truncados al leer u64"))?;
        self.position += 8;
        let mut raw = [0u8; 8];
        raw.copy_from_slice(slice);
        Ok(u64::from_le_bytes(raw))
    }

    /// Lee una cadena (prefijo `u32 LE` + UTF-8).
    fn read_str(&mut self) -> Result<String, RuscaError> {
        let length = self.read_u32()? as usize;
        let slice = self
            .bytes
            .get(self.position..self.position + length)
            .ok_or_else(|| corrupt("cadena del catálogo truncada"))?;
        self.position += length;
        String::from_utf8(slice.to_vec())
            .map_err(|_| corrupt("cadena del catálogo con UTF-8 inválido"))
    }
}

/// Deserializa el catálogo (los bytes sobrantes deben ser relleno cero).
fn decode_catalog(payload: &[u8]) -> Result<Catalog, RuscaError> {
    let mut decoder = Decoder::new(payload);
    let table_count = decoder.read_u32()?;
    let mut tables = BTreeMap::new();
    for _ in 0..table_count {
        let name = decoder.read_str()?;
        let table = decode_table(&mut decoder)?;
        tables.insert(name, table);
    }
    let next_page = decoder.read_u64()?;
    let trailing = &payload[decoder.position..];
    if trailing.iter().any(|byte| *byte != 0) {
        return Err(corrupt("bytes no nulos tras el catálogo"));
    }
    Ok(Catalog { tables, next_page })
}

/// Deserializa una tabla.
fn decode_table(decoder: &mut Decoder<'_>) -> Result<TableDef, RuscaError> {
    let name = decoder.read_str()?;
    let column_count = decoder.read_u32()?;
    let mut columns = Vec::with_capacity(column_count.min(1024) as usize);
    for _ in 0..column_count {
        columns.push(ColumnDef {
            name: decoder.read_str()?,
            col_type: decode_column_type(decoder.read_u8()?)?,
        });
    }
    let heap_start = PageId(decoder.read_u64()?);
    let page_count = decoder.read_u32()?;
    let mut pages = Vec::with_capacity(page_count.min(1_000_000) as usize);
    for _ in 0..page_count {
        pages.push(decoder.read_u64()?);
    }
    let row_count = decoder.read_u64()?;
    let has_index = decoder.read_u8()?;
    let index = match has_index {
        0 => None,
        1 => Some(decode_index(decoder)?),
        other => return Err(corrupt(&format!("marca de índice inválida: {other}"))),
    };
    Ok(TableDef {
        name,
        columns,
        heap_start,
        pages,
        row_count,
        index,
    })
}

/// Deserializa el índice de una tabla.
fn decode_index(decoder: &mut Decoder<'_>) -> Result<IndexDef, RuscaError> {
    let column = decoder.read_str()?;
    let page_count = decoder.read_u32()?;
    let mut pages = Vec::with_capacity(page_count.min(1_000_000) as usize);
    for _ in 0..page_count {
        pages.push(decoder.read_u64()?);
    }
    Ok(IndexDef { column, pages })
}

/// Traduce una etiqueta binaria a tipo de columna.
fn decode_column_type(tag: u8) -> Result<ColumnType, RuscaError> {
    match tag {
        0 => Ok(ColumnType::Bool),
        1 => Ok(ColumnType::Int),
        2 => Ok(ColumnType::Float),
        3 => Ok(ColumnType::Text),
        other => Err(corrupt(&format!("tipo de columna inválido: {other}"))),
    }
}
