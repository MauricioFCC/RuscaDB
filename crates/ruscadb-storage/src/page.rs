//! Páginas de tamaño fijo (4 KiB) y su identificador.

use std::fmt;

/// Tamaño de página de RuscaDB (alineado a 4 KiB, ver ADR-009/§5.6).
pub const PAGE_SIZE: usize = 4096;

/// Identificador de página dentro de un `PagedFile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

/// Página de tamaño fijo. El buffer se aloja en el heap (`Box`) para que
/// mover una `Page` sea barato (un puntero) y no copie 4 KiB.
#[derive(Clone, PartialEq, Eq)]
pub struct Page {
    id: PageId,
    data: Box<[u8; PAGE_SIZE]>,
}

impl Page {
    /// Crea una página nueva (en ceros) con el identificador dado.
    ///
    /// Args:
    ///     id: Identificador de la página.
    ///
    /// Returns:
    ///     Una página con los 4 KiB inicializados a cero.
    pub fn new(id: PageId) -> Self {
        Self {
            id,
            data: Box::new([0u8; PAGE_SIZE]),
        }
    }

    /// Identificador de la página.
    pub fn id(&self) -> PageId {
        self.id
    }

    /// Vista inmutable del contenido de la página.
    pub fn data(&self) -> &[u8; PAGE_SIZE] {
        &self.data
    }

    /// Vista mutable del contenido de la página.
    pub fn data_mut(&mut self) -> &mut [u8; PAGE_SIZE] {
        &mut self.data
    }
}

impl fmt::Debug for Page {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Page(id={}, size={})", self.id.0, PAGE_SIZE)
    }
}
