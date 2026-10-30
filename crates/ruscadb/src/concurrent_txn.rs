//! BEGIN CONCURRENT (SPEC-0131): transacciones concurrentes reales.
//!
//! Delega en [`ruscadb_txn::TxnManager`]: cada transaccion concurrente captura
//! un `snapshot_tx` y declara sus claves; el commit aplica
//! **first-committer-wins** (si otra transaccion confirmo la misma clave
//! despues de nuestro snapshot, devuelve `WriteConflict`).

use std::collections::{BTreeMap, BTreeSet};

use ruscadb_core::RuscaError;
use ruscadb_txn::TxnManager;

/// Transaccion concurrente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcurrentTxn {
    /// Identificador propio (monotono, asignado por `TxnManager`).
    pub tx_id: u64,
    /// Snapshot declarado por el llamador (informativo).
    ///
    /// La deteccion de conflictos usa el reloj interno de `TxnManager`
    /// (`begin_clock`), no este campo; se conserva por compatibilidad de API.
    pub snapshot_tx: u64,
}

/// Gestor de transacciones concurrentes con deteccion de conflicto.
#[derive(Debug, Clone)]
pub struct ConcurrentTxnManager {
    inner: TxnManager,
    writes: BTreeMap<u64, BTreeSet<String>>,
}

impl Default for ConcurrentTxnManager {
    fn default() -> Self {
        Self {
            inner: TxnManager::new(),
            writes: BTreeMap::new(),
        }
    }
}

impl ConcurrentTxnManager {
    /// Crea el gestor.
    ///
    /// Returns:
    ///     Gestor sin transacciones.
    pub fn new() -> Self {
        Self::default()
    }

    /// Abre una transaccion concurrente sobre `snapshot_tx`.
    ///
    /// Args:
    ///     snapshot_tx: Snapshot de lectura declarado por el llamador
    ///         (informativo; el conflicto se evalua con el reloj del
    ///         `TxnManager`).
    ///
    /// Returns:
    ///     Transaccion con `tx_id` unico del `TxnManager`.
    pub fn begin_concurrent(&mut self, snapshot_tx: u64) -> ConcurrentTxn {
        let tx_id = self.inner.begin();
        self.writes.insert(tx_id, BTreeSet::new());
        ConcurrentTxn { tx_id, snapshot_tx }
    }

    /// Declara una escritura de la transaccion.
    ///
    /// Args:
    ///     tx: Transaccion abierta.
    ///     key: Clave logica.
    ///
    /// Returns:
    ///     `Ok(())` si la transaccion esta en vuelo.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la transaccion no existe.
    pub fn stage_write(&mut self, tx: ConcurrentTxn, key: &str) -> Result<(), RuscaError> {
        self.inner.stage_write(tx.tx_id, key.to_string())?;
        self.writes
            .entry(tx.tx_id)
            .or_default()
            .insert(key.to_string());
        Ok(())
    }

    /// Confirma la transaccion (first-committer-wins).
    ///
    /// Args:
    ///     tx: Transaccion a confirmar.
    ///
    /// Returns:
    ///     `Ok(())` si no hubo conflicto.
    ///
    /// Errors:
    ///     [`RuscaError::WriteConflict`] si otra transaccion confirmo una de
    ///     nuestras claves desde el snapshot.
    pub fn commit(&mut self, tx: ConcurrentTxn) -> Result<(), RuscaError> {
        let result = self.inner.commit(tx.tx_id);
        if result.is_ok() {
            self.writes.remove(&tx.tx_id);
        }
        result
    }

    /// Aborta la transaccion.
    ///
    /// Args:
    ///     tx: Transaccion en vuelo.
    ///
    /// Returns:
    ///     `Ok(())` si existia.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si no estaba en vuelo.
    pub fn rollback(&mut self, tx: ConcurrentTxn) -> Result<(), RuscaError> {
        self.writes.remove(&tx.tx_id);
        self.inner.rollback(tx.tx_id)
    }

    /// Indica si una transaccion fue confirmada.
    ///
    /// Args:
    ///     tx_id: Identificador.
    ///
    /// Returns:
    ///     `true` si esta en el conjunto de commits.
    pub fn is_committed(&self, tx_id: u64) -> bool {
        self.inner.is_committed(tx_id)
    }

    /// Low watermark del `TxnManager`.
    ///
    /// Returns:
    ///     Ultima transaccion visible para todos.
    pub fn low_watermark(&self) -> u64 {
        self.inner.low_watermark()
    }

    /// Claves escritas por una transaccion (si sigue en vuelo).
    ///
    /// Args:
    ///     tx_id: Identificador.
    ///
    /// Returns:
    ///     Conjunto de claves, o vacio.
    pub fn writes(&self, tx_id: u64) -> Vec<String> {
        self.writes
            .get(&tx_id)
            .map(|keys| keys.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::ConcurrentTxnManager;
    use ruscadb_core::RuscaError;

    #[test] // @spec AC-0131-01
    fn test_ac_0131_01_disjoint_writers_commit() {
        let mut manager = ConcurrentTxnManager::new();
        let first = manager.begin_concurrent(0);
        let second = manager.begin_concurrent(0);
        assert_ne!(first.tx_id, second.tx_id);
        assert_eq!(first.snapshot_tx, second.snapshot_tx);

        // Escritores disjuntos: ambos confirman.
        manager.stage_write(first, "a").expect("stage a");
        manager.stage_write(second, "b").expect("stage b");
        manager.commit(first).expect("commit a");
        manager.commit(second).expect("commit b");
        assert!(manager.is_committed(first.tx_id));
        assert!(manager.is_committed(second.tx_id));
    }

    /// Dos escritores sobre la misma clave: el segundo aborta con conflicto.
    #[test]
    fn concurrent_write_conflict_is_detected() {
        let mut manager = ConcurrentTxnManager::new();
        let first = manager.begin_concurrent(0);
        let second = manager.begin_concurrent(0);
        manager.stage_write(first, "k").expect("stage 1");
        manager.stage_write(second, "k").expect("stage 2");
        manager.commit(first).expect("primero gana");
        assert!(matches!(
            manager.commit(second),
            Err(RuscaError::WriteConflict { .. })
        ));
        assert_eq!(manager.writes(first.tx_id), Vec::<String>::new());
    }

    /// Rollback libera la transaccion y no publica writes.
    #[test]
    fn rollback_discards_transaction() {
        let mut manager = ConcurrentTxnManager::new();
        let tx = manager.begin_concurrent(0);
        manager.stage_write(tx, "x").expect("stage");
        assert_eq!(manager.writes(tx.tx_id), vec!["x".to_string()]);
        manager.rollback(tx).expect("rollback");
        assert!(!manager.is_committed(tx.tx_id));
        assert!(manager.writes(tx.tx_id).is_empty());
        assert!(manager.stage_write(tx, "y").is_err(), "tx ya no existe");
    }
}
