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
///
/// Mantiene una **free-list** en memoria de páginas liberadas con
/// [`PagedFile::free_page`]: [`PagedFile::allocate`] reutiliza esas páginas
/// antes de crecer el fichero (patrón SQLite freelist / redb). La persistencia
/// de la free-list entre sesiones vive en el manifiesto de la base
/// (`ruscadb-txn`), que la fachada sincroniza; este tipo expone el primitivo de
/// asignación reutilizable y se prueba de forma aislada (SPEC-0085).
#[derive(Debug)]
pub struct PagedFile {
    file: File,
    path: PathBuf,
    pages: u64,
    /// Páginas liberadas y aún reutilizables (LIFO), nunca `>= pages`.
    free: Vec<PageId>,
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
        Ok(Self {
            file,
            path,
            pages,
            free: Vec::new(),
        })
    }

    /// Número de páginas del archivo (alto agua físico, no decrece; el
    /// truncado queda fuera de alcance).
    pub fn page_count(&self) -> u64 {
        self.pages
    }

    /// Número de páginas liberadas pendientes de reutilizar.
    pub fn free_count(&self) -> usize {
        self.free.len()
    }

    /// Copia de las páginas liberadas, en orden de liberación.
    ///
    /// Returns:
    ///     Los `PageId` reutilizables por [`PagedFile::allocate`].
    pub fn free_pages(&self) -> Vec<PageId> {
        self.free.clone()
    }

    /// Inyecta páginas liberadas persistidas (p. ej. cargadas del manifiesto).
    ///
    /// Ignora ids fuera de rango o ya presentes, de modo que la carga de una
    /// free-list persistida es idempotente y nunca introduce un doble free.
    ///
    /// Args:
    ///     pages: Identificadores de página (crudos) a marcar como libres.
    pub fn seed_free_pages(&mut self, pages: &[u64]) {
        for raw in pages {
            if *raw < self.pages && !self.free.iter().any(|id| id.0 == *raw) {
                self.free.push(PageId(*raw));
            }
        }
    }

    /// Marca `id` como libre para su reutilización (idempotente).
    ///
    /// No trunca el fichero: la página sigue ocupando su offset, pero
    /// [`PagedFile::allocate`] puede volver a entregarla. Un id ya libre es un
    /// *no-op* (sin doble free, NF-0085-02).
    ///
    /// Args:
    ///     id: Página a liberar; debe existir (`id < page_count`).
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si `id` está fuera del rango asignado
    ///     (WHAT: no existe la página; WHY: liberar una página nunca escrita
    ///     rompería el invariante de la free-list; WHERE: `paged_file.free_page`).
    pub fn free_page(&mut self, id: PageId) -> Result<(), RuscaError> {
        if id.0 >= self.pages {
            return Err(RuscaError::InvalidConfig(format!(
                "no se puede liberar la página {id:?}: está fuera del rango asignado ({} páginas)",
                self.pages
            )));
        }
        if !self.free.contains(&id) {
            self.free.push(id);
        }
        Ok(())
    }

    /// Ruta del archivo.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reserva una página (en ceros) reutilizando la free-list si hay.
    ///
    /// **WHAT**: si la free-list tiene páginas de [`PagedFile::free_page`],
    /// toma la última (LIFO) y la borra a ceros; si no, extiende el fichero.
    /// **WHY**: reutilizar antes de crecer mantiene estable el alto agua y evita
    /// que la base solo crezca. **WHERE**: `paged_file.allocate`.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura.
    pub fn allocate(&mut self) -> Result<PageId, RuscaError> {
        let id = self.free.pop().unwrap_or(PageId(self.pages));
        self.write_page(&Page::new(id))?;
        Ok(id)
    }

    /// Escribe una página en su offset, extendiendo el archivo si es necesario.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura.
    pub fn write_page(&mut self, page: &Page) -> Result<(), RuscaError> {
        let offset = page.id().0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(page.data())?;
        if page.id().0 >= self.pages {
            self.pages = page.id().0 + 1;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `allocate` reutiliza una página liberada antes de crecer el fichero.
    #[test]
    fn allocate_reuses_freed_page_before_growing() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("free.db");
        let mut file = PagedFile::open(&path).expect("open");
        let first = file.allocate().expect("alloc 0");
        let _second = file.allocate().expect("alloc 1");
        assert_eq!(file.page_count(), 2);

        file.free_page(first).expect("free");
        assert_eq!(file.free_count(), 1);
        let reused = file.allocate().expect("realloc");

        assert_eq!(reused, first, "se reutiliza la página liberada");
        assert_eq!(
            file.page_count(),
            2,
            "no crece el fichero al reutilizar una página libre"
        );
        assert_eq!(file.free_count(), 0, "la free-list se consume");
    }

    /// `free_page` es idempotente y rechaza ids fuera de rango.
    #[test]
    fn free_page_is_idempotent_and_validates_range() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("free-idem.db");
        let mut file = PagedFile::open(&path).expect("open");
        let page = file.allocate().expect("alloc");

        file.free_page(page).expect("primer free");
        file.free_page(page).expect("segundo free (no-op)");
        assert_eq!(file.free_count(), 1, "sin doble free");

        let out_of_range = file.free_page(PageId(99));
        assert!(
            matches!(out_of_range, Err(RuscaError::InvalidConfig(_))),
            "liberar una página inexistente falla accionablemente"
        );
    }

    /// `seed_free_pages` inyecta ids válidos y descarta inválidos/duplicados.
    #[test]
    fn seed_free_pages_filters_invalid_and_duplicates() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("free-seed.db");
        let mut file = PagedFile::open(&path).expect("open");
        file.allocate().expect("alloc 0");
        file.allocate().expect("alloc 1");

        file.seed_free_pages(&[1, 1, 99]);
        assert_eq!(
            file.free_pages(),
            vec![PageId(1)],
            "solo el id válido y único"
        );
    }
}
