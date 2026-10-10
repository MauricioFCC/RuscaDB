//! # ruscadb-storage
//!
//! Almacenamiento de RuscaDB: buffer pool con presupuesto RAM duro y desalojo
//! **LRU-K (K=2)**, más un archivo de páginas de tamaño fijo (`PagedFile`).
//!
//! Especificación: `specs/storage_engine.md` (SPEC-0003).
//! Diseño: `docs/RuscaDB-roadmap.md` §5.5/§5.6.

#![forbid(unsafe_code)]

mod buffer_pool;
mod page;
mod paged_file;

pub use buffer_pool::BufferPool;
pub use page::{PAGE_SIZE, Page, PageId};
pub use paged_file::PagedFile;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use ruscadb_core::RuscaError;
    use std::collections::HashMap;

    /// Mapa de páginas canónicas (marcador `data[0] = id`).
    fn canonical_pages() -> HashMap<PageId, Page> {
        (0..8)
            .map(|i| {
                let id = PageId(i);
                let mut page = Page::new(id);
                page.data_mut()[0] = i as u8;
                (id, page)
            })
            .collect()
    }

    /// Cargador que sirve páginas desde el mapa canónico.
    fn loader(pages: &HashMap<PageId, Page>) -> impl Fn(PageId) -> Result<Page, RuscaError> + '_ {
        move |id| {
            pages.get(&id).cloned().ok_or(RuscaError::PageOutOfRange {
                id: id.0,
                page_count: pages.len() as u64,
            })
        }
    }

    /// AC-0003-01 — el buffer pool respeta el presupuesto duro.
    #[test]
    // @spec AC-0003-01
    fn test_ac_0003_01_buffer_pool_respects_budget() {
        let pages = canonical_pages();
        let mut pool = BufferPool::new(2).expect("pool");
        for i in 0..5u64 {
            let id = PageId(i);
            let _ = pool.get(id, loader(&pages)).expect("get");
            pool.unpin(id, false).expect("unpin");
            assert!(pool.len() <= 2, "el pool excedió su capacidad");
        }
        assert_eq!(pool.len(), 2);
    }

    /// AC-0003-02 — una página pinneada no se desaloja (backpressure).
    #[test]
    // @spec AC-0003-02
    fn test_ac_0003_02_pinned_page_is_not_evicted() {
        let pages = canonical_pages();
        let mut pool = BufferPool::new(1).expect("pool");
        let _ = pool.get(PageId(0), loader(&pages)).expect("get");
        let error = pool.get(PageId(1), loader(&pages)).unwrap_err();
        assert!(matches!(error, RuscaError::BufferPoolFull { capacity: 1 }));
    }

    /// AC-0003-03 — una página desalojada se recarga desde el loader.
    #[test]
    // @spec AC-0003-03
    fn test_ac_0003_03_evicted_page_reloads() {
        let pages = canonical_pages();
        let mut pool = BufferPool::new(1).expect("pool");

        let _ = pool.get(PageId(0), loader(&pages)).expect("get");
        pool.unpin(PageId(0), false).expect("unpin");
        let _ = pool.get(PageId(1), loader(&pages)).expect("get");
        pool.unpin(PageId(1), false).expect("unpin");
        assert_eq!(pool.misses(), 2);

        let page = pool.get(PageId(0), loader(&pages)).expect("reload");
        assert_eq!(page.id(), PageId(0));
        assert_eq!(pool.misses(), 3);
    }

    /// AC-0003-04 — LRU-2 discrimina frecuencia (desaloja la de 1 referencia).
    #[test]
    // @spec AC-0003-04
    fn test_ac_0003_04_lru_k_discriminates() {
        let pages = canonical_pages();
        let mut pool = BufferPool::new(2).expect("pool");
        for id in [PageId(0), PageId(1), PageId(0)] {
            let _ = pool.get(id, loader(&pages)).expect("get");
            pool.unpin(id, false).expect("unpin");
        }
        let _ = pool.get(PageId(2), loader(&pages)).expect("get");
        pool.unpin(PageId(2), false).expect("unpin");

        assert!(pool.contains(PageId(0)), "A (2 refs) debe permanecer");
        assert!(!pool.contains(PageId(1)), "B (1 ref) debe desalojarse");
    }

    /// AC-0003-05 — roundtrip de `PagedFile`.
    #[test]
    // @spec AC-0003-05
    fn test_ac_0003_05_paged_file_roundtrip() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("heap.db");
        let mut written = Vec::new();
        {
            let mut file = PagedFile::open(&path).expect("open");
            for i in 0..3u8 {
                let id = file.allocate().expect("alloc");
                let mut page = Page::new(id);
                page.data_mut()[0] = i + 1;
                file.write_page(&page).expect("write");
                written.push((id, i + 1));
            }
            file.flush().expect("flush");
        }

        let mut file = PagedFile::open(&path).expect("reopen");
        assert_eq!(file.page_count(), 3);
        for (id, marker) in written {
            let page = file.read_page(id).expect("read");
            assert_eq!(page.data()[0], marker);
        }
    }

    /// Accesores, contadores y seguimiento de páginas sucias.
    #[test]
    fn test_buffer_pool_accessors_and_dirty_tracking() {
        let pages = canonical_pages();
        let mut pool = BufferPool::new(2).expect("pool");
        assert!(pool.is_empty());
        assert_eq!(pool.capacity(), 2);
        assert_eq!(pool.hits(), 0);

        let _ = pool.get(PageId(0), loader(&pages)).expect("get");
        pool.unpin(PageId(0), true).expect("unpin dirty");
        assert!(!pool.is_empty());
        assert_eq!(pool.is_dirty(PageId(0)), Some(true));
        assert_eq!(pool.dirty_pages().len(), 1);
        assert_eq!(pool.is_dirty(PageId(9)), None);

        let _ = pool.get(PageId(0), loader(&pages)).expect("hit");
        assert_eq!(pool.hits(), 1);
        pool.unpin(PageId(0), false).expect("unpin");
        pool.mark_clean(PageId(0)).expect("mark clean");
        assert_eq!(pool.is_dirty(PageId(0)), Some(false));
        assert!(pool.dirty_pages().is_empty());

        assert!(matches!(
            pool.unpin(PageId(9), false),
            Err(RuscaError::PageNotInPool { .. })
        ));
    }

    /// El `Debug` de `Page` identifica la página (no imprime 4 KiB).
    #[test]
    fn test_page_debug_is_informative() {
        let text = format!("{:?}", Page::new(PageId(3)));
        assert!(text.contains("Page"));
        assert!(text.contains('3'));
    }

    proptest! {
        /// Invariante: el buffer pool nunca supera su capacidad.
        #[test]
        fn prop_buffer_pool_never_exceeds_capacity(
            ops in prop::collection::vec((0u64..8, any::<bool>()), 1..200),
            capacity in 1usize..5,
        ) {
            let pages = canonical_pages();
            let mut pool = BufferPool::new(capacity).expect("pool");
            for (raw, dirty) in ops {
                let id = PageId(raw % 8);
                if pool.get(id, loader(&pages)).is_ok() {
                    let _ = pool.unpin(id, dirty);
                }
                prop_assert!(pool.len() <= capacity);
            }
        }

        /// Invariante: `PagedFile` preserva el contenido byte a byte.
        #[test]
        fn prop_paged_file_roundtrip(
            contents in prop::collection::vec(
                prop::collection::vec(any::<u8>(), 1..64),
                1..10,
            ),
        ) {
            let dir = tempfile::tempdir().expect("dir");
            let path = dir.path().join("heap.db");
            let mut written = Vec::new();
            {
                let mut file = PagedFile::open(&path).expect("open");
                for bytes in &contents {
                    let id = file.allocate().expect("alloc");
                    let mut page = Page::new(id);
                    page.data_mut()[..bytes.len()].copy_from_slice(bytes);
                    file.write_page(&page).expect("write");
                    written.push((id, bytes.clone()));
                }
                file.flush().expect("flush");
            }
            let mut file = PagedFile::open(&path).expect("reopen");
            for (id, bytes) in &written {
                let page = file.read_page(*id).expect("read");
                prop_assert_eq!(&page.data()[..bytes.len()], bytes.as_slice());
            }
        }
    }
}
