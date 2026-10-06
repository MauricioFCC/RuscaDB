//! Almacén de páginas de tamaño fijo en disco.
//!
//! Cada página ocupa exactamente [`PAGE_SIZE`] bytes en `offset = id * PAGE_SIZE`.
//! Ver `specs/storage_engine.md` (SPEC-0003).

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ruscadb_core::RuscaError;

use crate::page::{PAGE_SIZE, Page, PageId};

/// Archivo de páginas de tamaño fijo.
#[derive(Debug)]
pub struct PagedFile {
    file: File,
    path: PathBuf,
    pages: u64,
}

impl PagedFile {
    /// Abre (o crea) el archivo de páginas.
    ///
    /// Args:
    ///     path: Ruta del archivo de páginas.
    ///
    /// Returns:
    ///     El archivo abierto, con el número de páginas derivado del tamaño.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el acceso a disco.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RuscaError> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let pages = file.metadata()?.len() / PAGE_SIZE as u64;
        Ok(Self { file, path, pages })
    }

    /// Número de páginas del archivo.
    pub fn page_count(&self) -> u64 {
        self.pages
    }

    /// Ruta del archivo.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reserva una página nueva (en ceros) al final y devuelve su id.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura.
    pub fn allocate(&mut self) -> Result<PageId, RuscaError> {
        let id = PageId(self.pages);
        self.write_page(&Page::new(id))?;
        self.pages += 1;
        Ok(id)
    }

    /// Escribe una página en su offset.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura.
    pub fn write_page(&mut self, page: &Page) -> Result<(), RuscaError> {
        let offset = page.id().0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(page.data())?;
        Ok(())
    }

    /// Lee una página por su id.
    ///
    /// Errors:
    ///     [`RuscaError::PageOutOfRange`] si el id no existe;
    ///     [`RuscaError::Io`] si falla la lectura.
    pub fn read_page(&mut self, id: PageId) -> Result<Page, RuscaError> {
        if id.0 >= self.pages {
            return Err(RuscaError::PageOutOfRange {
                id: id.0,
                page_count: self.pages,
            });
        }
        let offset = id.0 * PAGE_SIZE as u64;
        let mut page = Page::new(id);
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(page.data_mut())?;
        Ok(page)
    }

    /// Fuerza la durabilidad de todo lo escrito (fsync).
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el fsync.
    pub fn flush(&mut self) -> Result<(), RuscaError> {
        self.file.sync_data()?;
        Ok(())
    }
}
