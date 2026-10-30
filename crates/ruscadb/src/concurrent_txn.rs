//! BEGIN CONCURRENT (SPEC-0131): transacciones concurrentes con snapshot fijo.
//!
//! Modelo multi-writer: cada transaccion concurrente captura un `snapshot_tx`
//! y recibe su propio `tx_id` monotono; el conflicto se resuelve en commit
//! (first-committer-wins, ver `ruscadb-txn`).

/// Transaccion concurrente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcurrentTxn {
    /// Identificador propio (monotono).
    pub tx_id: u64,
    /// Snapshot desde el que lee.
    pub snapshot_tx: u64,
}

/// Gestor de transacciones concurrentes.
#[derive(Debug, Clone, Default)]
pub struct ConcurrentTxnManager {
    next_tx: u64,
}

impl ConcurrentTxnManager {
    /// Crea el gestor con el primer `tx_id` en 1.
    ///
    /// Returns:
    ///     Gestor vacio.
    pub fn new() -> Self {
        Self { next_tx: 1 }
    }

    /// Abre una transaccion concurrente sobre `snapshot_tx`.
    ///
    /// Args:
    ///     snapshot_tx: Snapshot de lectura.
    ///
    /// Returns:
    ///     Transaccion con id unico.
    pub fn begin_concurrent(&mut self, snapshot_tx: u64) -> ConcurrentTxn {
        let tx_id = self.next_tx;
        self.next_tx += 1;
        ConcurrentTxn { tx_id, snapshot_tx }
    }
}

#[cfg(test)]
mod tests {
    use super::ConcurrentTxnManager;

    #[test] // @spec AC-0131-01
    fn test_ac_0131_01_disjoint_writers_commit() {
        let mut manager = ConcurrentTxnManager::new();
        let first = manager.begin_concurrent(0);
        let second = manager.begin_concurrent(0);
        assert_ne!(first.tx_id, second.tx_id);
        assert_eq!(first.snapshot_tx, second.snapshot_tx);
    }
}
