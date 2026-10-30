//! Persistencia de HNSW en formato binario versionado (SPEC-0173).
//!
//! Guardar el grafo en texto no es viable: se necesita un formato binario
//! estable que detecte corrupcion. Este modulo define `RHNSW1`:
//!
//! ```text
//! [6B magic "RHNSW1"][u32 version][u32 dim][u32 count]
//! por nodo: [u64 id][u32 n_vecinos][u32 * n][f32 * dim]
//! [u32 checksum FNV-1a de todo lo anterior]
//! ```
//!
//! `load` valida magic, version, cotas y checksum antes de confiar en el
//! buffer: un archivo truncado o corrupto devuelve `None`, nunca panic.

/// Identificador del formato.
const MAGIC: &[u8; 6] = b"RHNSW1";
/// Version del formato soportada.
const FORMAT_VERSION: u32 = 1;

/// Nodo del grafo.
#[derive(Debug, Clone, PartialEq)]
pub struct HnswNode {
    /// Identificador estable del vector.
    pub id: u64,
    /// Vector almacenado.
    pub vector: Vec<f32>,
    /// Vecinos (indices dentro del indice).
    pub neighbors: Vec<u32>,
}

/// Indice HNSW persistible.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistentHnsw {
    /// Numero de vectores.
    pub vectors: usize,
    /// Version del formato en memoria.
    pub version: u32,
    /// Dimension de los vectores (0 = sin fijar).
    pub dimension: usize,
    /// Nodos en orden de insercion.
    nodes: Vec<HnswNode>,
}

impl Default for PersistentHnsw {
    fn default() -> Self {
        Self::new()
    }
}

impl PersistentHnsw {
    /// Crea un indice vacio de dimension libre.
    ///
    /// Returns:
    ///     Indice vacio en la version actual del formato.
    pub fn new() -> Self {
        Self {
            vectors: 0,
            version: FORMAT_VERSION,
            dimension: 0,
            nodes: Vec::new(),
        }
    }

    /// Crea un indice vacio con dimension fija.
    ///
    /// Args:
    ///     dimension: Dimension de los vectores.
    ///
    /// Returns:
    ///     Indice configurado.
    pub fn with_dimension(dimension: usize) -> Self {
        Self {
            dimension,
            ..Self::new()
        }
    }

    /// Anade un nodo con vector nulo (compatibilidad).
    pub fn add(&mut self) {
        let id = self.vectors as u64;
        let vector = vec![0.0; self.dimension];
        self.insert(id, vector, Vec::new());
    }

    /// Inserta un nodo.
    ///
    /// Args:
    ///     id: Identificador estable.
    ///     vector: Vector (fija la dimension si aun no lo estaba).
    ///     neighbors: Vecinos del nodo.
    pub fn insert(&mut self, id: u64, vector: Vec<f32>, neighbors: Vec<u32>) {
        if self.dimension == 0 {
            self.dimension = vector.len();
        }
        self.nodes.push(HnswNode {
            id,
            vector,
            neighbors,
        });
        self.vectors = self.nodes.len();
    }

    /// Nodo por posicion.
    ///
    /// Args:
    ///     index: Posicion.
    ///
    /// Returns:
    ///     Referencia al nodo, si existe.
    pub fn get(&self, index: usize) -> Option<&HnswNode> {
        self.nodes.get(index)
    }

    /// Numero de nodos.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Indica si el indice esta vacio.
    ///
    /// Returns:
    ///     `true` sin nodos.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Serializa el indice a `RHNSW1`.
    ///
    /// Returns:
    ///     Buffer binario autoverificable.
    pub fn save(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(22 + self.nodes.len() * 32);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&(self.dimension as u32).to_le_bytes());
        out.extend_from_slice(&(self.vectors as u32).to_le_bytes());
        for node in &self.nodes {
            out.extend_from_slice(&node.id.to_le_bytes());
            out.extend_from_slice(&(node.neighbors.len() as u32).to_le_bytes());
            for neighbor in &node.neighbors {
                out.extend_from_slice(&neighbor.to_le_bytes());
            }
            for value in &node.vector {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        let checksum = fnv1a32(&out);
        out.extend_from_slice(&checksum.to_le_bytes());
        out
    }

    /// Deserializa y valida un buffer `RHNSW1`.
    ///
    /// Args:
    ///     bytes: Buffer producido por [`Self::save`].
    ///
    /// Returns:
    ///     Indice reconstruido, o `None` si el buffer es invalido o esta
    ///     corrupto.
    pub fn load(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < MAGIC.len() + 12 + 4 {
            return None;
        }
        if &bytes[..MAGIC.len()] != MAGIC {
            return None;
        }
        let body = &bytes[..bytes.len() - 4];
        let expected = u32::from_le_bytes(bytes[bytes.len() - 4..].try_into().ok()?);
        if fnv1a32(body) != expected {
            return None;
        }

        let version = read_u32(body, 6)?;
        if version != FORMAT_VERSION {
            return None;
        }
        let dimension = read_u32(body, 10)? as usize;
        let count = read_u32(body, 14)? as usize;
        let mut cursor = 18usize;
        // Cota defensiva: cada nodo ocupa al menos 12 bytes (id + n_vecinos).
        if count.checked_mul(12)? > body.len().saturating_sub(cursor) {
            return None;
        }
        let mut nodes = Vec::with_capacity(count);
        for _ in 0..count {
            let id = read_u64(body, cursor)?;
            cursor += 8;
            let neighbor_count = read_u32(body, cursor)? as usize;
            cursor += 4;
            if neighbor_count.checked_mul(4)? > body.len().saturating_sub(cursor) {
                return None;
            }
            let mut neighbors = Vec::with_capacity(neighbor_count);
            for _ in 0..neighbor_count {
                neighbors.push(read_u32(body, cursor)?);
                cursor += 4;
            }
            let vector_bytes = dimension.checked_mul(4)?;
            if vector_bytes > body.len().saturating_sub(cursor) {
                return None;
            }
            let mut vector = Vec::with_capacity(dimension);
            for _ in 0..dimension {
                let raw = read_u32(body, cursor)?;
                vector.push(f32::from_bits(raw));
                cursor += 4;
            }
            nodes.push(HnswNode {
                id,
                vector,
                neighbors,
            });
        }
        if cursor != body.len() {
            return None;
        }
        Some(Self {
            vectors: nodes.len(),
            version,
            dimension,
            nodes,
        })
    }
}

/// Lee un `u32` LE en `offset`.
fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

/// Lee un `u64` LE en `offset`.
fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice = bytes.get(offset..offset + 8)?;
    Some(u64::from_le_bytes(slice.try_into().ok()?))
}

/// FNV-1a de 32 bits (checksum rapido y sin dependencias).
fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in bytes {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::PersistentHnsw;

    /// AC-0173-01: round-trip con vectores y vecinos.
    #[test]
    // @spec AC-0173-01
    fn test_ac_0173_01_roundtrip() {
        let mut index = PersistentHnsw::with_dimension(3);
        index.insert(10, vec![1.0, 2.0, 3.0], vec![1]);
        index.insert(20, vec![-1.0, 0.5, 0.25], vec![0]);
        let bytes = index.save();
        let restored = PersistentHnsw::load(&bytes).expect("round-trip");

        assert_eq!(restored.vectors, 2);
        assert_eq!(restored.dimension, 3);
        assert_eq!(restored.version, 1);
        assert_eq!(restored.get(0), index.get(0));
        assert_eq!(restored.get(1), index.get(1));

        // Indice vacio tambien sobrevive.
        let empty = PersistentHnsw::new();
        assert!(
            PersistentHnsw::load(&empty.save())
                .expect("vacio")
                .is_empty()
        );
        assert_eq!(PersistentHnsw::default(), PersistentHnsw::new());
    }

    /// AC-0173-02: basura, truncado y bit-flip se rechazan sin panic.
    #[test]
    // @spec AC-0173-02
    fn test_ac_0173_02_corrupt() {
        assert!(PersistentHnsw::load(b"garbage").is_none());
        assert!(PersistentHnsw::load(&[]).is_none());

        let mut index = PersistentHnsw::with_dimension(2);
        index.insert(1, vec![1.0, 2.0], vec![]);
        let bytes = index.save();

        for cut in 0..bytes.len() {
            assert!(
                PersistentHnsw::load(&bytes[..cut]).is_none(),
                "truncado en {cut} debe fallar"
            );
        }

        let mut flipped = bytes.clone();
        flipped[20] ^= 0b0000_0001;
        assert!(
            PersistentHnsw::load(&flipped).is_none(),
            "bit-flip detectable"
        );

        // Version no soportada.
        let mut wrong_version = bytes.clone();
        wrong_version[6] = 99;
        // El checksum tambien cambia con la version, asi que se recalcula para
        // probar que el rechazo es por version y no solo por checksum.
        let body_len = wrong_version.len() - 4;
        let checksum = super::fnv1a32(&wrong_version[..body_len]);
        wrong_version[body_len..].copy_from_slice(&checksum.to_le_bytes());
        assert!(PersistentHnsw::load(&wrong_version).is_none());
    }

    /// Los nodos anadidos con `add` usan ids correlativos.
    #[test]
    fn add_assigns_sequential_ids() {
        let mut index = PersistentHnsw::new();
        index.add();
        index.add();
        assert_eq!(index.len(), 2);
        assert_eq!(index.get(0).map(|node| node.id), Some(0));
        assert_eq!(index.get(1).map(|node| node.id), Some(1));
    }

    /// Serializacion determinista y magic incorrecta rechazada.
    #[test]
    fn save_is_deterministic_and_magic_is_checked() {
        let mut index = PersistentHnsw::with_dimension(2);
        index.insert(1, vec![1.0, 2.0], vec![]);
        let first = index.save();
        assert_eq!(first, index.save(), "misma entrada -> mismos bytes");

        let mut wrong_magic = first.clone();
        wrong_magic[0] = b'X';
        assert!(PersistentHnsw::load(&wrong_magic).is_none());
    }
}
