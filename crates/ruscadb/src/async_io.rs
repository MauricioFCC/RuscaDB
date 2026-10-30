//! I/O asincrono sin Tokio (SPEC-0151): lector sincrono reutilizable.
//!
//! El offload a un hilo del executor lo hace el consumidor; aqui se expone el
//! acceso acotado (`offset + len`) sin bloquear el runtime.

/// Lector en memoria con lectura acotada.
#[derive(Debug, Clone)]
pub struct AsyncReader {
    /// Bytes disponibles.
    pub data: Vec<u8>,
}

impl AsyncReader {
    /// Crea el lector.
    ///
    /// Args:
    ///     data: Bytes a leer.
    ///
    /// Returns:
    ///     Lector listo.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Lee `len` bytes desde `offset` (vacio fuera de rango).
    ///
    /// Args:
    ///     offset: Posicion inicial.
    ///     len: Numero de bytes.
    ///
    /// Returns:
    ///     Copia de los bytes disponibles; vacio si el rango no cabe.
    pub fn read_sync(&self, offset: usize, len: usize) -> Vec<u8> {
        self.data
            .get(offset..offset.saturating_add(len))
            .unwrap_or(&[])
            .to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::AsyncReader;

    #[test] // @spec AC-0151-01
    fn test_ac_0151_01_async_read() {
        let reader = AsyncReader::new(vec![1, 2, 3, 4, 5]);
        assert_eq!(reader.read_sync(1, 3), vec![2, 3, 4]);
        assert!(reader.read_sync(9, 3).is_empty());
    }
}
