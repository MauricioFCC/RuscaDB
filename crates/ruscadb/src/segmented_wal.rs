//! WAL segmentado con LSN y checkpoint (SPEC-0168).
//!
//! El WAL crece sin limite si es un unico archivo: la rotacion por segmentos
//! permite hacer checkpoint (truncar lo ya aplicado) sin reescribir todo. Cada
//! registro recibe un **LSN monotono** que identifica su posicion logica; la
//! recuperacion reproduce los registros en orden de LSN.

/// Registro del WAL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalRecord {
    /// Numero de secuencia del log (monotono, empieza en 1).
    pub lsn: u64,
    /// Carga util del registro.
    pub payload: Vec<u8>,
}

/// WAL segmentado en memoria (modelo de la politica de rotacion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentedWal {
    /// Tamano objetivo de cada segmento en bytes.
    pub segment_size: usize,
    /// Segmentos, del mas antiguo al mas reciente.
    segments: Vec<Vec<WalRecord>>,
    /// Bytes ocupados por cada segmento.
    sizes: Vec<usize>,
    /// Siguiente LSN a asignar.
    next_lsn: u64,
}

impl SegmentedWal {
    /// Crea el WAL con un segmento vacio.
    ///
    /// Args:
    ///     segment_size: Bytes objetivo por segmento (0 se interpreta como 1).
    ///
    /// Returns:
    ///     WAL con un unico segmento.
    pub fn new(segment_size: usize) -> Self {
        Self {
            segment_size: segment_size.max(1),
            segments: vec![Vec::new()],
            sizes: vec![0],
            next_lsn: 1,
        }
    }

    /// Anade un registro y devuelve su LSN.
    ///
    /// Args:
    ///     payload: Carga util.
    ///
    /// Returns:
    ///     LSN asignado (monotono).
    pub fn append(&mut self, payload: &[u8]) -> u64 {
        // Rota ANTES de escribir: el registro nuevo pertenece al segmento
        // nuevo, de modo que el checkpoint conserva el ultimo registro.
        let last = self.segments.len() - 1;
        if self.sizes[last] >= self.segment_size && !self.segments[last].is_empty() {
            self.segments.push(Vec::new());
            self.sizes.push(0);
        }
        let lsn = self.next_lsn;
        self.next_lsn += 1;
        let record = WalRecord {
            lsn,
            payload: payload.to_vec(),
        };
        let last = self.segments.len() - 1;
        self.segments[last].push(record);
        self.sizes[last] += payload.len();
        lsn
    }

    /// Rota si el segmento actual alcanzo el tamano indicado.
    ///
    /// Args:
    ///     current_size: Bytes actuales del segmento.
    ///
    /// Returns:
    ///     `true` si se creo un segmento nuevo.
    pub fn rotate_if_needed(&mut self, current_size: usize) -> bool {
        if current_size >= self.segment_size {
            self.segments.push(Vec::new());
            self.sizes.push(0);
            true
        } else {
            false
        }
    }

    /// Hace checkpoint: descarta los segmentos cerrados y conserva el actual.
    ///
    /// Returns:
    ///     LSN del ultimo registro conservado (0 si no hay registros).
    pub fn checkpoint(&mut self) -> u64 {
        let last_records = self.segments.pop().unwrap_or_default();
        let last_size = self.sizes.pop().unwrap_or(0);
        let last_lsn = last_records.last().map(|record| record.lsn).unwrap_or(0);
        self.segments = vec![last_records];
        self.sizes = vec![last_size];
        last_lsn
    }

    /// Numero de segmentos.
    ///
    /// Returns:
    ///     Segmentos actuales (incluye el abierto).
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Bytes totales almacenados.
    ///
    /// Returns:
    ///     Suma de cargas utiles.
    pub fn total_bytes(&self) -> usize {
        self.sizes.iter().sum()
    }

    /// Todos los registros en orden de LSN.
    ///
    /// Returns:
    ///     Copia de los registros.
    pub fn records(&self) -> Vec<WalRecord> {
        self.segments.iter().flatten().cloned().collect()
    }

    /// Recupera concatenando las cargas utiles en orden.
    ///
    /// Returns:
    ///     Bytes de replay.
    pub fn recover(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_bytes());
        for record in self.segments.iter().flatten() {
            out.extend_from_slice(&record.payload);
        }
        out
    }

    /// Segmento que contiene un LSN.
    ///
    /// Args:
    ///     lsn: Numero de secuencia.
    ///
    /// Returns:
    ///     Indice de segmento, o `None` si el LSN no esta presente.
    pub fn segment_of(&self, lsn: u64) -> Option<usize> {
        self.segments
            .iter()
            .position(|segment| segment.iter().any(|record| record.lsn == lsn))
    }
}

#[cfg(test)]
mod tests {
    use super::SegmentedWal;

    /// AC-0168-01: la rotacion crea un segmento nuevo.
    #[test]
    // @spec AC-0168-01
    fn test_ac_0168_01_rotate() {
        let mut wal = SegmentedWal::new(10);
        assert!(wal.rotate_if_needed(10));
        assert_eq!(wal.segment_count(), 2);
        assert!(!wal.rotate_if_needed(9));
        assert_eq!(wal.segment_count(), 2);
    }

    /// AC-0168-02: el checkpoint descarta segmentos cerrados.
    #[test]
    // @spec AC-0168-02
    fn test_ac_0168_02_checkpoint() {
        let mut wal = SegmentedWal::new(4);
        wal.append(b"aaa");
        wal.append(b"bbb");
        // El tercer registro dispara la rotacion (el segmento ya tenia 6 B).
        wal.append(b"ccc");
        assert!(wal.segment_count() >= 2);
        let last = wal.checkpoint();
        assert_eq!(wal.segment_count(), 1);
        assert_eq!(last, 3, "conserva el ultimo LSN");
        assert_eq!(wal.recover(), b"ccc");
    }

    /// AC-0168-03: LSN monotono y replay en orden (group commit).
    #[test]
    // @spec AC-0168-03
    fn test_ac_0168_03_group_commit() {
        let mut wal = SegmentedWal::new(8);
        let mut lsns = Vec::new();
        for index in 0..100u64 {
            lsns.push(wal.append(format!("r{index}").as_bytes()));
        }
        assert_eq!(lsns, (1..=100).collect::<Vec<u64>>());
        assert!(lsns.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(wal.segment_of(1), Some(0));
        // "r0".."r9" miden 2 bytes (10 registros) y "r10".."r99" miden 3 (90).
        assert_eq!(wal.recover().len(), 10 * 2 + 90 * 3);
        // El replay empieza por el primer registro.
        assert!(wal.recover().starts_with(b"r0"));
    }

    /// Regresion de mutantes (linea 58): NO se rota por debajo del tamano
    /// objetivo, ni aunque el segmento tenga datos (`&&` vs `||`, `>=` vs `<`).
    #[test]
    fn append_does_not_rotate_below_target() {
        let mut wal = SegmentedWal::new(10);
        wal.append(b"a");
        wal.append(b"b");
        assert_eq!(wal.segment_count(), 1, "2 bytes < 10 no rotan");
        assert_eq!(wal.recover(), b"ab");

        // Un segmento vacio nunca dispara la rotacion (sizes[last] = 0).
        let mut empty = SegmentedWal::new(10);
        empty.append(b"x");
        empty.checkpoint();
        assert_eq!(empty.segment_count(), 1);
        empty.append(b"y");
        assert_eq!(empty.segment_count(), 1);
    }

    /// El segmento exactamente lleno rota en la siguiente escritura.
    #[test]
    fn append_rotates_at_exact_boundary() {
        let mut wal = SegmentedWal::new(4);
        wal.append(b"abcd");
        assert_eq!(wal.segment_count(), 1, "rota antes de escribir, no despues");
        wal.append(b"e");
        assert_eq!(wal.segment_count(), 2);
        assert_eq!(wal.segment_of(1), Some(0));
        assert_eq!(wal.segment_of(2), Some(1));
    }
}
