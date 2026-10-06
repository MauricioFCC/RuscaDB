//! Buffer pool con presupuesto de memoria duro y desalojo LRU-K (K=2).
//!
//! Ver `specs/storage_engine.md` (SPEC-0003) y `docs/RuscaDB-roadmap.md` §5.6.
//! Política: O'Neil et al., "The LRU-K Page Replacement Algorithm" (1993).

use std::collections::{HashMap, VecDeque};

use ruscadb_core::RuscaError;

use crate::page::{Page, PageId};

/// Orden K del LRU-K (se consideran las últimas K referencias por página).
const K: usize = 2;

/// Marco del buffer pool.
struct Frame {
    page: Page,
    pin_count: u32,
    dirty: bool,
    valid: bool,
    /// Últimas K marcas de tiempo lógicas de referencia (la más reciente al final).
    k_refs: VecDeque<u64>,
}

impl Frame {
    /// Marco vacío (aún sin página cargada).
    fn empty() -> Self {
        Self {
            page: Page::new(PageId(0)),
            pin_count: 0,
            dirty: false,
            valid: false,
            k_refs: VecDeque::new(),
        }
    }

    /// Marco con una página recién cargada (queda pinneada por el lector).
    fn with_page(page: Page, clock: u64) -> Self {
        let mut k_refs = VecDeque::with_capacity(K);
        k_refs.push_back(clock);
        Self {
            page,
            pin_count: 1,
            dirty: false,
            valid: true,
            k_refs,
        }
    }

    /// Registra una referencia, conservando solo las últimas K.
    fn record_ref(&mut self, clock: u64) {
        self.k_refs.push_back(clock);
        while self.k_refs.len() > K {
            self.k_refs.pop_front();
        }
    }

    /// Marca de referencia más reciente (0 si nunca).
    fn last_ref(&self) -> u64 {
        self.k_refs.back().copied().unwrap_or(0)
    }

    /// Distancia de retroceso K (u64::MAX si hay menos de K referencias).
    fn kth_distance(&self, now: u64) -> u64 {
        let count = self.k_refs.len();
        if count < K {
            u64::MAX
        } else {
            now.saturating_sub(self.k_refs[count - K])
        }
    }
}

/// Buffer pool de páginas con presupuesto RAM duro (`capacity * 4 KiB`).
pub struct BufferPool {
    frames: Vec<Frame>,
    index: HashMap<PageId, usize>,
    clock: u64,
    capacity: usize,
    hits: u64,
    misses: u64,
}

impl BufferPool {
    /// Crea un buffer pool con la capacidad dada (número de marcos).
    ///
    /// Args:
    ///     capacity: Número máximo de marcos en memoria (>= 1).
    ///
    /// Returns:
    ///     El pool, o [`RuscaError::InvalidConfig`] si `capacity == 0`.
    pub fn new(capacity: usize) -> Result<Self, RuscaError> {
        if capacity == 0 {
            return Err(RuscaError::InvalidConfig(
                "la capacidad del buffer pool debe ser >= 1".to_string(),
            ));
        }
        Ok(Self {
            frames: Vec::with_capacity(capacity),
            index: HashMap::new(),
            clock: 0,
            capacity,
            hits: 0,
            misses: 0,
        })
    }

    /// Capacidad configurada (número de marcos).
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Número de páginas actualmente en memoria.
    pub fn len(&self) -> usize {
        self.frames.iter().filter(|frame| frame.valid).count()
    }

    /// `true` si no hay páginas cargadas.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Aciertos de caché acumulados.
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Fallos de caché acumulados.
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// `true` si la página está en memoria.
    pub fn contains(&self, id: PageId) -> bool {
        self.index.contains_key(&id)
    }

    /// Estado `dirty` de una página en memoria (`None` si no está).
    pub fn is_dirty(&self, id: PageId) -> Option<bool> {
        self.index.get(&id).map(|&idx| self.frames[idx].dirty)
    }

    /// Devuelve un clon de todas las páginas sucias en memoria.
    pub fn dirty_pages(&self) -> Vec<Page> {
        self.frames
            .iter()
            .filter(|frame| frame.valid && frame.dirty)
            .map(|frame| frame.page.clone())
            .collect()
    }

    /// Marca una página como limpia (tras escribirla a disco).
    ///
    /// Errors:
    ///     [`RuscaError::PageNotInPool`] si la página no está en memoria.
    pub fn mark_clean(&mut self, id: PageId) -> Result<(), RuscaError> {
        let Some(&idx) = self.index.get(&id) else {
            return Err(RuscaError::PageNotInPool { id: id.0 });
        };
        self.frames[idx].dirty = false;
        Ok(())
    }

    /// Obtiene una página, cargándola con `load` si no está en memoria.
    ///
    /// Deja la página **pinneada**; el llamador debe invocar [`BufferPool::unpin`].
    ///
    /// Args:
    ///     id: Página solicitada.
    ///     load: Cargador usado en un fallo de caché.
    ///
    /// Returns:
    ///     Referencia a la página pinneada.
    ///
    /// Errors:
    ///     [`RuscaError::BufferPoolFull`] si no hay marcos desalojables.
    pub fn get<F>(&mut self, id: PageId, load: F) -> Result<&Page, RuscaError>
    where
        F: FnOnce(PageId) -> Result<Page, RuscaError>,
    {
        let idx = self.ensure_loaded(id, load)?;
        Ok(&self.frames[idx].page)
    }

    /// Igual que [`BufferPool::get`] pero devuelve una referencia mutable a la
    /// página (para escritura in-place en el marco).
    ///
    /// Errors:
    ///     [`RuscaError::BufferPoolFull`] si no hay marcos desalojables.
    pub fn get_mut<F>(&mut self, id: PageId, load: F) -> Result<&mut Page, RuscaError>
    where
        F: FnOnce(PageId) -> Result<Page, RuscaError>,
    {
        let idx = self.ensure_loaded(id, load)?;
        Ok(&mut self.frames[idx].page)
    }

    /// Garantiza que `id` está cargada y pinneada; devuelve el índice del marco.
    fn ensure_loaded<F>(&mut self, id: PageId, load: F) -> Result<usize, RuscaError>
    where
        F: FnOnce(PageId) -> Result<Page, RuscaError>,
    {
        self.clock += 1;

        if let Some(&idx) = self.index.get(&id) {
            self.hits += 1;
            let frame = &mut self.frames[idx];
            frame.pin_count += 1;
            frame.record_ref(self.clock);
            return Ok(idx);
        }

        self.misses += 1;
        let idx = if self.frames.len() < self.capacity {
            self.frames.push(Frame::empty());
            self.frames.len() - 1
        } else {
            self.victim_index().ok_or(RuscaError::BufferPoolFull {
                capacity: self.capacity,
            })?
        };

        if self.frames[idx].valid {
            let evicted = self.frames[idx].page.id();
            self.index.remove(&evicted);
        }
        let page = load(id)?;
        self.frames[idx] = Frame::with_page(page, self.clock);
        self.index.insert(id, idx);
        Ok(idx)
    }

    /// Despinnea una página y opcionalmente la marca como sucia.
    ///
    /// Errors:
    ///     [`RuscaError::PageNotInPool`] si la página no está o no está pinneada.
    pub fn unpin(&mut self, id: PageId, dirty: bool) -> Result<(), RuscaError> {
        let Some(&idx) = self.index.get(&id) else {
            return Err(RuscaError::PageNotInPool { id: id.0 });
        };
        let frame = &mut self.frames[idx];
        if frame.pin_count == 0 {
            return Err(RuscaError::PageNotInPool { id: id.0 });
        }
        frame.pin_count -= 1;
        frame.dirty |= dirty;
        Ok(())
    }

    /// Selecciona el marco a desalojar (LRU-K entre marcos limpios y no pinneados).
    ///
    /// Se elige la mayor distancia de retroceso K (menos usado); a igualdad,
    /// la referencia más antigua. Los marcos con < K referencias tienen
    /// distancia infinita y se desalojan primero.
    fn victim_index(&self) -> Option<usize> {
        self.frames
            .iter()
            .enumerate()
            .filter(|(_, frame)| frame.valid && frame.pin_count == 0 && !frame.dirty)
            .min_by_key(|(_, frame)| {
                (
                    std::cmp::Reverse(frame.kth_distance(self.clock)),
                    frame.last_ref(),
                )
            })
            .map(|(idx, _)| idx)
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn frame_with(clock: u64) -> Frame {
        Frame::with_page(Page::new(PageId(0)), clock)
    }

    /// `record_ref` conserva solo las últimas K referencias.
    #[test]
    fn frame_record_ref_keeps_last_k() {
        let mut frame = frame_with(1);
        frame.record_ref(2);
        frame.record_ref(3);
        frame.record_ref(4);
        assert_eq!(frame.k_refs.len(), K);
        assert_eq!(frame.last_ref(), 4);
    }

    /// `last_ref` devuelve la referencia más reciente.
    #[test]
    fn frame_last_ref_is_latest() {
        assert_eq!(frame_with(7).last_ref(), 7);
    }

    /// `kth_distance` usa la K-ésima referencia (no la última).
    #[test]
    fn frame_kth_distance_uses_kth_reference() {
        let mut frame = frame_with(1);
        frame.record_ref(3);
        // ahora=10, k_refs=[1,3] -> distancia K=2 = 10 - 1 = 9
        assert_eq!(frame.kth_distance(10), 9);
    }

    /// `kth_distance` es infinita con menos de K referencias.
    #[test]
    fn frame_kth_distance_infinite_below_k() {
        assert_eq!(frame_with(1).kth_distance(10), u64::MAX);
    }

    /// `get` avanza el reloj lógico (base de las distancias LRU-K).
    #[test]
    fn get_advances_logical_clock() {
        let mut pool = BufferPool::new(1).expect("pool");
        let _ = pool.get(PageId(0), |id| Ok(Page::new(id))).expect("get");
        assert_eq!(pool.clock, 1);
    }
}
