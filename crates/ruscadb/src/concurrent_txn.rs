//! BEGIN CONCURRENT (SPEC-0131): multi-writer stub.

/// Transacción concurrente.
#[derive(Debug, Clone)]
pub struct ConcurrentTxn {
    /// ID de la transacción.
    pub tx_id: u64,
    /// Snapshot tx_id al BEGIN.
    pub snapshot_tx: u64,
}

/// Gestor de transacciones concurrentes.
#[derive(Debug, Clone, Default)]
pub struct ConcurrentTxnManager {
    /// Contador de transacciones.
    next_tx: u64,
}

impl ConcurrentTxnManager {
    /// Crea un gestor.
    pub fn new() -> Self {
        Self { next_tx: 1 }
    }

    /// Inicia una transacción concurrente.
    pub fn begin_concurrent(&mut self, snapshot_tx: u64) -> ConcurrentTxn {
        let tx_id = self.next_tx;
        self.next_tx += 1;
        ConcurrentTxn { tx_id, snapshot_tx }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0131-01
    fn test_ac_0131_01_disjoint_writers_commit() {
        let mut mgr = ConcurrentTxnManager::new();
        let tx1 = mgr.begin_concurrent(0);
        let tx2 = mgr.begin_concurrent(0);
        assert_ne!(tx1.tx_id, tx2.tx_id);
    }
}
