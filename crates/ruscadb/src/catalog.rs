//! Catálogo persistente de tablas (SPEC-0012, FR-0012-01; SPEC-0086).
//!
//! El [`Catalog`] mapea nombres de tabla a su [`TableDef`] y actúa como
//! asignador *bump* de páginas (`next_page`, sin free-list). Se persiste a
//! partir de la **página 0**.
//!
//! ## Formato legacy (SPEC-0031, versión 1)
//!
//! Superblock de 16 bytes `[magic u64 LE | versión u32 LE | páginas de catálogo
//! u32 LE]` seguido de los bytes del catálogo, que se extienden de forma
//! **contigua** a `0..catalog_pages`. La región reservada [`CATALOG_RESERVED_PAGES`]
//! (16 páginas = 64 KiB) acota el catálogo; el heap empieza después.
//!
//! ## Formato paginado (SPEC-0086, versión 2)
//!
//! Cuando el payload no cabe en la región reservada, se aplica el **patrón
//! overflow de SPEC-0071**: las páginas extra se pagan con el mismo asignador
//! *bump* (`next_page`, coherente con el heap) y se enlazan en una cadena
//! `[next u64 LE | chunk]`. El superblock v2 (24 bytes) reutiliza el campo de
//! páginas como **número de páginas de overflow** y añade `overflow_first u64`.
//! Como ambos asignadores avanzan el mismo `next_page`, las páginas del catálogo
//! y del heap nunca colisionan. El techo práctico desaparece (queda el máximo de
//! `u32` páginas de overflow, ~16 TiB, con error accionable).
//!
//! ## Versiones 3/4 (SPEC-0104, multi-índice)
//!
//! El payload de cada tabla guarda la **lista** de índices secundarios en lugar
//! de un único índice opcional. La versión **3** conserva el layout reservado
//! (superblock de 16 bytes, ≤ 64 KiB) y la **4** el layout paginado con cadena
//! de overflow (superblock de 24 bytes); ambas son la escritura por defecto. El
//! decodificador sigue leyendo las versiones **1** y **2** (un índice por tabla,
//! marcador `u8`), por lo que una base persistida con el formato anterior carga
//! sin migración; una versión desconocida produce [`RuscaError::CorruptManifest`]
//! accionable en vez de leer datos a ciegas.
//!
//! El códec es manual (enteros LE y cadenas con prefijo `u32 LE`) para no
//! añadir dependencias de serialización a la fachada.

use std::collections::BTreeMap;

use ruscadb_core::RuscaError;
use ruscadb_storage::{PAGE_SIZE, Page, PageId};

use crate::Database;

/// Magia del superblock (`"RUSCDB01"`): distingue la página 0 del catálogo.
pub(crate) const CATALOG_MAGIC: u64 = 0x5255_5343_4442_3031;

/// Versión legacy del formato del catálogo persistido (región reservada contigua
/// con **un** índice por tabla, SPEC-0031). Sigue siendo leída (SPEC-0104).
pub(crate) const CATALOG_VERSION: u32 = 1;

/// Versión legacy del formato paginado con cadena de overflow y **un** índice
/// por tabla (SPEC-0086). Sigue siendo leída (SPEC-0104).
const CATALOG_VERSION_OVERFLOW: u32 = 2;

/// Versión del formato reservado con **varios** índices por tabla (SPEC-0104).
/// Mismo layout que v1 (superblock de 16 bytes) y payload con lista de índices.
const CATALOG_VERSION_MULTI: u32 = 3;

/// Versión del formato paginado con overflow y **varios** índices por tabla
/// (SPEC-0104). Mismo layout que v2 (superblock de 24 bytes + cadena).
const CATALOG_VERSION_MULTI_OVERFLOW: u32 = 4;

/// Tamaño del superblock v1 en bytes (magic u64 + versión u32 + páginas u32).
const SUPERBLOCK_SIZE: usize = 16;

/// Tamaño del superblock v2: v1 + `overflow_first` (u64 LE en `16..24`).
const SUPERBLOCK_V2_SIZE: usize = 24;

/// Páginas reservadas al catálogo (`0..16` = 64 KiB, cientos de tablas).
const CATALOG_RESERVED_PAGES: u64 = 16;

/// Bytes de payload que caben en la región reservada con superblock v2.
const RESERVED_PAYLOAD_V2: usize = CATALOG_RESERVED_PAGES as usize * PAGE_SIZE - SUPERBLOCK_V2_SIZE;

/// Cabecera de una página de overflow del catálogo: `next u64 LE`.
const OVERFLOW_HEADER: usize = 8;

/// Bytes útiles de payload por página de overflow del catálogo.
const OVERFLOW_CHUNK: usize = PAGE_SIZE - OVERFLOW_HEADER;

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

/// Definición de un índice secundario de una tabla (una columna, SPEC-0012
/// §FR-0012-03; SPEC-0104 admite varios por tabla).
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
    /// Índices secundarios de la tabla (uno por columna indexada, SPEC-0104).
    pub indexes: Vec<IndexDef>,
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
    /// Despacha según la versión del superblock: v1/v3 (región reservada,
    /// SPEC-0031/SPEC-0104) o v2/v4 (payload paginado con cadena de overflow,
    /// SPEC-0086/SPEC-0104); las versiones v1/v2 (un índice por tabla) se
    /// leen sin migración. Si la página 0 no existe o no contiene la magia,
    /// devuelve un catálogo vacío (base nueva).
    ///
    /// Args:
    ///     database: Base abierta.
    ///
    /// Errors:
    ///     [`RuscaError::CorruptManifest`] si el superblock o la cadena de
    ///     páginas del catálogo son inválidos.
    pub(crate) fn load(database: &mut Database) -> Result<Self, RuscaError> {
        if !database.page_exists(PageId(0)) {
            return Ok(Self::new());
        }
        let first = database.read_page(PageId(0))?;
        if read_magic(first.data()) != CATALOG_MAGIC {
            return Ok(Self::new());
        }
        match read_version(first.data()) {
            CATALOG_VERSION_OVERFLOW => load_overflow(database, &first, false),
            CATALOG_VERSION_MULTI_OVERFLOW => load_overflow(database, &first, true),
            CATALOG_VERSION => load_reserved(database, &first, false),
            CATALOG_VERSION_MULTI => load_reserved(database, &first, true),
            other => Err(corrupt(&format!(
                "versión de superblock desconocida: {other} (esperaba {CATALOG_VERSION}..\
                 {CATALOG_VERSION_MULTI_OVERFLOW}); abre la base con una versión compatible"
            ))),
        }
    }

    /// Persiste el catálogo y confirma (WAL-first).
    ///
    /// Con ≤ 16 páginas escribe el formato reservado **v3** (superblock de
    /// 16 bytes, multi-índice, SPEC-0104) en la región reservada. Si el payload
    /// la supera, paga páginas extra con el mismo asignador *bump* (`next_page`)
    /// y las enlaza en una cadena de overflow (patrón SPEC-0071, SPEC-0086) con
    /// el formato paginado **v4**. Incluye en el mismo commit las páginas
    /// sucias previas (heap/índice), por lo que `insert` logra atomicidad con
    /// una sola llamada.
    ///
    /// Args:
    ///     database: Base abierta.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el número de páginas de overflow
    ///     excede `u32` (techo residual ~16 TiB, irrealizable); el mensaje
    ///     explica la causa y el límite;
    ///     [`RuscaError::Io`] si falla la escritura o la confirmación.
    pub(crate) fn save(&mut self, database: &mut Database) -> Result<(), RuscaError> {
        if self.next_page < CATALOG_RESERVED_PAGES {
            self.next_page = CATALOG_RESERVED_PAGES;
        }
        let bytes = encode_catalog(self);
        if bytes.len() <= RESERVED_PAYLOAD_V2 {
            return write_reserved(database, &bytes);
        }
        // Payload grande: se necesitan páginas de overflow pagadas con el
        // asignador *bump* compartido con el heap (nunca colisionan).
        let needed = div_ceil(bytes.len() - RESERVED_PAYLOAD_V2, OVERFLOW_CHUNK);
        let mut chain = existing_overflow_chain(database)?;
        while chain.len() < needed {
            chain.push(self.alloc_page().0);
        }
        // `next_page` pudo cambiar al asignar; el tamaño codificado no (u64 fijo).
        let bytes = encode_catalog(self);
        write_overflow(database, &bytes, &chain[..needed])
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
                indexes: Vec::new(),
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

    /// Índice secundario sobre una columna, si existe (SPEC-0104).
    ///
    /// Args:
    ///     column: Nombre de la columna.
    ///
    /// Returns:
    ///     El [`IndexDef`] de esa columna, o `None` si no está indexada.
    pub(crate) fn index_for(&self, column: &str) -> Option<&IndexDef> {
        self.indexes.iter().find(|index| index.column == column)
    }

    /// Columnas con índice secundario, en orden de creación.
    ///
    /// Returns:
    ///     Una cadena por cada índice de la tabla.
    pub fn index_columns(&self) -> Vec<String> {
        self.indexes
            .iter()
            .map(|index| index.column.clone())
            .collect()
    }

    /// Páginas del índice de una columna (vacío si esa columna no tiene índice).
    ///
    /// Args:
    ///     column: Nombre de la columna.
    pub(crate) fn index_pages(&self, column: &str) -> Vec<u64> {
        self.index_for(column)
            .map(|index| index.pages.clone())
            .unwrap_or_default()
    }

    /// Sustituye las páginas del índice de una columna (no-op si no existe).
    ///
    /// Args:
    ///     column: Nombre de la columna.
    ///     pages: Nuevas páginas del índice.
    pub(crate) fn set_index_pages(&mut self, column: &str, pages: Vec<u64>) {
        if let Some(index) = self.indexes.iter_mut().find(|index| index.column == column) {
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
///
/// En v1 son las páginas contiguas del catálogo; en v2, el número de páginas
/// de la cadena de overflow (mismo offset `12..16`).
fn read_catalog_pages(data: &[u8]) -> u32 {
    let mut pages = [0u8; 4];
    pages.copy_from_slice(&data[12..16]);
    u32::from_le_bytes(pages)
}

/// Lee la versión del formato del superblock (`8..12`).
fn read_version(data: &[u8]) -> u32 {
    let mut version = [0u8; 4];
    version.copy_from_slice(&data[8..12]);
    u32::from_le_bytes(version)
}

/// Lee el puntero `overflow_first` de un superblock v2 (`16..24`).
fn read_overflow_first(data: &[u8]) -> u64 {
    let mut first = [0u8; 8];
    first.copy_from_slice(&data[16..24]);
    u64::from_le_bytes(first)
}

/// Lee el puntero `next` de una página de overflow del catálogo (`0..8`).
fn read_next(data: &[u8]) -> u64 {
    let mut next = [0u8; 8];
    next.copy_from_slice(&data[0..8]);
    u64::from_le_bytes(next)
}

/// Carga un catálogo reservado (contiguo, `0..catalog_pages`).
///
/// Args:
///     database: Base abierta.
///     first: Página 0 ya leída (contenía la magia).
///     multi: `true` si el payload guarda varios índices por tabla (v3,
///         SPEC-0104); `false` para el formato un índice (v1, SPEC-0031).
///
/// Returns:
///     El catálogo decodificado.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el superblock declara 0 páginas o
///     falta una página declarada.
fn load_reserved(
    database: &mut Database,
    first: &Page,
    multi: bool,
) -> Result<Catalog, RuscaError> {
    let catalog_pages = read_catalog_pages(first.data()) as u64;
    if catalog_pages == 0 {
        return Err(corrupt("el superblock declara 0 páginas de catálogo"));
    }
    let mut payload = Vec::with_capacity(catalog_pages as usize * PAGE_SIZE);
    for raw in 0..catalog_pages {
        let page = read_catalog_page(database, raw, catalog_pages)?;
        let skip = if raw == 0 { SUPERBLOCK_SIZE } else { 0 };
        payload.extend_from_slice(&page.data()[skip..]);
    }
    decode_catalog(&payload, multi)
}

/// Carga un catálogo paginado: región reservada + cadena de páginas de overflow.
///
/// Args:
///     database: Base abierta.
///     first: Página 0 ya leída (superblock de 24 bytes, v2 o v4).
///     multi: `true` si el payload guarda varios índices por tabla (v4,
///         SPEC-0104); `false` para el formato un índice (v2, SPEC-0086).
///
/// Returns:
///     El catálogo decodificado.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si falta una página reservada, la cadena
///     de overflow se trunca o el payload no decodifica.
fn load_overflow(
    database: &mut Database,
    first: &Page,
    multi: bool,
) -> Result<Catalog, RuscaError> {
    let overflow_pages = read_catalog_pages(first.data()) as usize;
    let mut payload = Vec::with_capacity(RESERVED_PAYLOAD_V2 + overflow_pages * OVERFLOW_CHUNK);
    payload.extend_from_slice(&first.data()[SUPERBLOCK_V2_SIZE..]);
    for raw in 1..CATALOG_RESERVED_PAGES {
        let page = database
            .read_page(PageId(raw))
            .map_err(|error| match error {
                RuscaError::PageOutOfRange { .. } => corrupt(&format!(
                    "el catálogo v2 reserva {CATALOG_RESERVED_PAGES} páginas pero falta la {raw}"
                )),
                other => other,
            })?;
        payload.extend_from_slice(page.data());
    }
    let mut current = read_overflow_first(first.data());
    for index in 0..overflow_pages {
        if current == 0 {
            return Err(corrupt(&format!(
                "la cadena de overflow del catálogo declara {overflow_pages} páginas pero \
                 termina en la {index}"
            )));
        }
        let page = read_overflow_page(database, current)?;
        payload.extend_from_slice(&page.data()[OVERFLOW_HEADER..]);
        current = read_next(page.data());
    }
    decode_catalog(&payload, multi)
}

/// Lee la cadena de overflow existente (v2/v4) desde la página 0.
///
/// Devuelve vacío si la base es nueva, no tiene magia o guarda un catálogo de
/// región reservada (v1/v3). Se usa en `save` para reutilizar las páginas de
/// overflow ya asignadas y no filtrar una página por cada escritura (sin
/// free-list, SPEC-0086).
///
/// Args:
///     database: Base abierta.
///
/// Returns:
///     Los `PageId` de la cadena, en orden.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si la cadena se trunca antes del número
///     declarado de páginas de overflow.
fn existing_overflow_chain(database: &mut Database) -> Result<Vec<u64>, RuscaError> {
    if !database.page_exists(PageId(0)) {
        return Ok(Vec::new());
    }
    let first = database.read_page(PageId(0))?;
    if read_magic(first.data()) != CATALOG_MAGIC
        || !matches!(
            read_version(first.data()),
            CATALOG_VERSION_OVERFLOW | CATALOG_VERSION_MULTI_OVERFLOW
        )
    {
        return Ok(Vec::new());
    }
    let count = read_catalog_pages(first.data()) as usize;
    let mut chain = Vec::with_capacity(count);
    let mut current = read_overflow_first(first.data());
    for index in 0..count {
        if current == 0 {
            return Err(corrupt(&format!(
                "la cadena de overflow del catálogo se truncó en la página {index} de {count}"
            )));
        }
        chain.push(current);
        let page = read_overflow_page(database, current)?;
        current = read_next(page.data());
    }
    Ok(chain)
}

/// Lee una página del catálogo mapeando la ausencia a un error accionable.
///
/// Args:
///     database: Base abierta.
///     raw: Índice de la página.
///     declared: Número de páginas declarado (para el mensaje).
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si la página no existe.
fn read_catalog_page(database: &mut Database, raw: u64, declared: u64) -> Result<Page, RuscaError> {
    database
        .read_page(PageId(raw))
        .map_err(|error| match error {
            RuscaError::PageOutOfRange { .. } => corrupt(&format!(
                "el catálogo declara {declared} páginas pero falta la {raw}"
            )),
            other => other,
        })
}

/// Lee una página de la cadena de overflow mapeando la ausencia a error.
///
/// Args:
///     database: Base abierta.
///     page_id: `PageId` de la página de overflow.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si la página no existe.
fn read_overflow_page(database: &mut Database, page_id: u64) -> Result<Page, RuscaError> {
    database
        .read_page(PageId(page_id))
        .map_err(|error| match error {
            RuscaError::PageOutOfRange { .. } => corrupt(&format!(
                "el catálogo apunta a la página de overflow {page_id}, que no existe"
            )),
            other => other,
        })
}

/// Escribe el formato reservado **v3** (contiguo, multi-índice) y confirma.
///
/// Args:
///     database: Base abierta.
///     bytes: Payload codificado (`<= RESERVED_PAYLOAD_V2`).
///
/// Errors:
///     [`RuscaError::Io`] si falla la escritura o la confirmación.
fn write_reserved(database: &mut Database, bytes: &[u8]) -> Result<(), RuscaError> {
    let pages = div_ceil(SUPERBLOCK_SIZE + bytes.len(), PAGE_SIZE);
    let total = pages * PAGE_SIZE;
    let mut buffer = vec![0u8; total];
    buffer[0..8].copy_from_slice(&CATALOG_MAGIC.to_le_bytes());
    buffer[8..12].copy_from_slice(&CATALOG_VERSION_MULTI.to_le_bytes());
    buffer[12..16].copy_from_slice(&(pages as u32).to_le_bytes());
    buffer[SUPERBLOCK_SIZE..SUPERBLOCK_SIZE + bytes.len()].copy_from_slice(bytes);
    for (index, chunk) in buffer.chunks(PAGE_SIZE).enumerate() {
        let mut page = Page::new(PageId(index as u64));
        page.data_mut().copy_from_slice(chunk);
        database.write_page(&page)?;
    }
    database.commit()?;
    Ok(())
}

/// Escribe el formato paginado **v4**: 16 páginas reservadas + cadena de
/// overflow (multi-índice), y confirma.
///
/// Args:
///     database: Base abierta.
///     bytes: Payload codificado (supera `RESERVED_PAYLOAD_V2`).
///     chain: Páginas de overflow (no vacía) que almacenan el resto del payload.
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si hay más páginas de overflow que `u32`
///     (techo residual irrealizable);
///     [`RuscaError::Io`] si falla la escritura o la confirmación.
fn write_overflow(database: &mut Database, bytes: &[u8], chain: &[u64]) -> Result<(), RuscaError> {
    let count = u32::try_from(chain.len()).map_err(|_| {
        RuscaError::InvalidConfig(format!(
            "el catálogo necesita {} páginas de overflow y el formato admite {} (u32); \
             reduce tablas/columnas o divide el esquema",
            chain.len(),
            u32::MAX
        ))
    })?;
    let reserved_payload = RESERVED_PAYLOAD_V2.min(bytes.len());
    let mut buffer = vec![0u8; CATALOG_RESERVED_PAGES as usize * PAGE_SIZE];
    buffer[0..8].copy_from_slice(&CATALOG_MAGIC.to_le_bytes());
    buffer[8..12].copy_from_slice(&CATALOG_VERSION_MULTI_OVERFLOW.to_le_bytes());
    buffer[12..16].copy_from_slice(&count.to_le_bytes());
    buffer[16..24].copy_from_slice(&chain[0].to_le_bytes());
    buffer[SUPERBLOCK_V2_SIZE..SUPERBLOCK_V2_SIZE + reserved_payload]
        .copy_from_slice(&bytes[..reserved_payload]);
    for (index, chunk) in buffer.chunks(PAGE_SIZE).enumerate() {
        let mut page = Page::new(PageId(index as u64));
        page.data_mut().copy_from_slice(chunk);
        database.write_page(&page)?;
    }
    let mut offset = reserved_payload;
    for (index, page_id) in chain.iter().enumerate() {
        let remaining = &bytes[offset..];
        let take = remaining.len().min(OVERFLOW_CHUNK);
        let next = chain.get(index + 1).copied().unwrap_or(0);
        let mut page = Page::new(PageId(*page_id));
        page.data_mut()[0..OVERFLOW_HEADER].copy_from_slice(&next.to_le_bytes());
        page.data_mut()[OVERFLOW_HEADER..OVERFLOW_HEADER + take]
            .copy_from_slice(&remaining[..take]);
        database.write_page(&page)?;
        offset += take;
    }
    database.commit()?;
    Ok(())
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
    push_u32(buffer, table.indexes.len() as u32);
    for index in &table.indexes {
        push_str(buffer, &index.column);
        push_u32(buffer, index.pages.len() as u32);
        for page in &index.pages {
            push_u64(buffer, *page);
        }
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
///
/// Args:
///     payload: Bytes del payload (región reservada + overflow concatenados).
///     multi: `true` para el formato multi-índice (v3/v4, SPEC-0104); `false`
///         para el legacy de un índice (v1/v2).
///
/// Returns:
///     El catálogo decodificado.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el payload está truncado, tiene
///     marcas/tipos inválidos o bytes no nulos al final.
fn decode_catalog(payload: &[u8], multi: bool) -> Result<Catalog, RuscaError> {
    let mut decoder = Decoder::new(payload);
    let table_count = decoder.read_u32()?;
    let mut tables = BTreeMap::new();
    for _ in 0..table_count {
        let name = decoder.read_str()?;
        let table = decode_table(&mut decoder, multi)?;
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
///
/// Args:
///     decoder: Lector con comprobación de límites.
///     multi: `true` para el formato multi-índice (v3/v4); `false` para el
///         legacy de un índice (v1/v2, marcador `u8`).
///
/// Returns:
///     La definición de la tabla.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el payload está truncado o inválido.
fn decode_table(decoder: &mut Decoder<'_>, multi: bool) -> Result<TableDef, RuscaError> {
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
    let indexes = if multi {
        let index_count = decoder.read_u32()?;
        let mut indexes = Vec::with_capacity(index_count.min(1024) as usize);
        for _ in 0..index_count {
            indexes.push(decode_index(decoder)?);
        }
        indexes
    } else {
        // Formato legacy (SPEC-0031/0086): 0 = sin índice, 1 = su único índice.
        match decoder.read_u8()? {
            0 => Vec::new(),
            1 => vec![decode_index(decoder)?],
            other => return Err(corrupt(&format!("marca de índice inválida: {other}"))),
        }
    };
    Ok(TableDef {
        name,
        columns,
        heap_start,
        pages,
        row_count,
        indexes,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Payload legacy (v1/v2, un índice) escrito a mano con el marcador `u8`.
    ///
    /// Args:
    ///     marker: `0` sin índice, `1` con el índice `idx` (columna `id`).
    ///
    /// Returns:
    ///     El payload `table_count | tabla | next_page` con el formato antiguo.
    fn legacy_payload(marker: u8) -> Vec<u8> {
        let mut buffer = Vec::new();
        push_u32(&mut buffer, 1);
        push_str(&mut buffer, "t");
        push_str(&mut buffer, "t");
        push_u32(&mut buffer, 2);
        push_str(&mut buffer, "id");
        buffer.push(column_tag(ColumnType::Int));
        push_str(&mut buffer, "name");
        buffer.push(column_tag(ColumnType::Text));
        push_u64(&mut buffer, 16);
        push_u32(&mut buffer, 1);
        push_u64(&mut buffer, 16);
        push_u64(&mut buffer, 0);
        buffer.push(marker);
        if marker == 1 {
            push_str(&mut buffer, "id");
            push_u32(&mut buffer, 1);
            push_u64(&mut buffer, 20);
        }
        push_u64(&mut buffer, CATALOG_RESERVED_PAGES);
        buffer
    }

    /// Roundtrip multi-índice (SPEC-0104): varios índices por tabla sobreviven.
    #[test]
    // @spec AC-0104-05
    fn test_ac_0104_05_catalog_roundtrip_multi_index() {
        let mut catalog = Catalog::new();
        catalog
            .register_table(
                "t",
                vec![ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                }],
            )
            .expect("register");
        catalog.tables.get_mut("t").expect("tabla").indexes = vec![
            IndexDef {
                column: "id".to_string(),
                pages: vec![16, 17],
            },
            IndexDef {
                column: "name".to_string(),
                pages: vec![18],
            },
        ];
        let bytes = encode_catalog(&catalog);
        let decoded = decode_catalog(&bytes, true).expect("decode multi");
        assert_eq!(decoded, catalog, "roundtrip multi-índice exacto");
        let table = decoded.get("t").expect("tabla");
        assert_eq!(table.index_columns(), vec!["id", "name"]);
        assert_eq!(table.index_pages("id"), vec![16, 17]);
        assert_eq!(table.index_pages("name"), vec![18]);
        assert!(table.index_pages("missing").is_empty());
    }

    /// Compatibilidad (SPEC-0104): el payload legacy sin índice (marcador 0)
    /// decodifica como tabla sin índices.
    #[test]
    // @spec AC-0104-06
    fn test_ac_0104_06_legacy_payload_without_index_loads() {
        let catalog = decode_catalog(&legacy_payload(0), false).expect("decode legacy");
        let table = catalog.get("t").expect("tabla");
        assert!(table.indexes.is_empty(), "marcador 0 = sin índice");
        assert_eq!(table.row_count, 0);
        assert_eq!(table.columns.len(), 2);
    }

    /// Compatibilidad (SPEC-0104): el payload legacy con su único índice
    /// (marcador 1) decodifica como un índice de una columna.
    #[test]
    // @spec AC-0104-07
    fn test_ac_0104_07_legacy_payload_with_single_index_loads() {
        let catalog = decode_catalog(&legacy_payload(1), false).expect("decode legacy");
        let table = catalog.get("t").expect("tabla");
        assert_eq!(table.index_columns(), vec!["id"], "marcador 1 = un índice");
        assert_eq!(table.index_pages("id"), vec![20]);
    }

    /// Un marcador de índice legacy inválido es un error accionable.
    #[test]
    // @spec AC-0104-08
    fn test_ac_0104_08_legacy_invalid_index_marker_is_actionable() {
        let error = decode_catalog(&legacy_payload(7), false).expect_err("marcador 7");
        assert!(
            error.to_string().contains("marca de índice inválida"),
            "mensaje accionable: {error}"
        );
    }
}
