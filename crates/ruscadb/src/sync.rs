//! Protocolo de sincronizacion entre replicas (SPEC-0138).
//!
//! El checkpoint (LSN + epoch) se resuelve con un CRDT last-writer-wins con
//! reloj vectorial ([`CrdtSyncReal`], SPEC-0179): dos replicas que han visto lo
//! mismo convergen al mismo epoch sin coordinacion, y `merge` es conmutativo e
//! idempotente. El LSN converge por maximo (monotono).

use std::collections::BTreeMap;

use crate::crdt_sync_real::CrdtSyncReal;

/// Estado de sincronizacion de una replica.
#[derive(Debug, Clone)]
pub struct SyncProtocol {
    /// Ultimo LSN comunmente confirmado.
    pub checkpoint_lsn: u64,
    /// Epoch vigente (resuelto por el CRDT).
    pub epoch: u64,
    /// Reloj vectorial + valor LWW del epoch.
    replica: CrdtSyncReal,
}

impl SyncProtocol {
    /// Crea el protocolo con identidad `"local"`.
    ///
    /// Returns:
    ///     Protocolo vacio (LSN 0, epoch 0).
    pub fn new() -> Self {
        Self {
            checkpoint_lsn: 0,
            epoch: 0,
            replica: CrdtSyncReal::new(),
        }
    }

    /// Crea el protocolo con identidad de replica explicita.
    ///
    /// Args:
    ///     actor: Nombre unico de la replica.
    ///
    /// Returns:
    ///     Protocolo vacio con ese actor.
    pub fn with_replica(actor: impl Into<String>) -> Self {
        Self {
            checkpoint_lsn: 0,
            epoch: 0,
            replica: CrdtSyncReal::with_actor(actor),
        }
    }

    /// Registra un checkpoint local.
    ///
    /// Args:
    ///     lsn: Ultimo LSN confirmado.
    ///     epoch: Epoch del checkpoint.
    pub fn checkpoint(&mut self, lsn: u64, epoch: u64) {
        self.checkpoint_lsn = self.checkpoint_lsn.max(lsn);
        self.epoch = epoch;
        self.replica.set(epoch as i64);
    }

    /// Fusiona el estado de otra replica.
    ///
    /// Args:
    ///     remote: Protocolo remoto.
    ///
    /// Returns:
    ///     Epoch resultante de la fusion.
    pub fn merge(&mut self, remote: &Self) -> u64 {
        self.replica.merge(&remote.replica);
        self.checkpoint_lsn = self.checkpoint_lsn.max(remote.checkpoint_lsn);
        self.epoch = self
            .replica
            .value()
            .unwrap_or(0)
            .max(0)
            .try_into()
            .unwrap_or(u64::MAX);
        self.epoch
    }

    /// Reloj vectorial actual (actores vistos y sus contadores).
    ///
    /// Returns:
    ///     Mapa ordenado `actor -> contador`.
    pub fn vector_clock(&self) -> &BTreeMap<String, u64> {
        self.replica.vector_clock()
    }
}

impl Default for SyncProtocol {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::SyncProtocol;

    /// AC-0138-01: el checkpoint local actualiza LSN y epoch.
    #[test]
    // @spec AC-0138-01
    fn test_ac_0138_01_sync_checkpoint() {
        let mut sync = SyncProtocol::new();
        sync.checkpoint(42, 7);
        assert_eq!(sync.checkpoint_lsn, 42);
        assert_eq!(sync.epoch, 7);
    }

    /// El merge converge y es conmutativo (CRDT).
    #[test]
    fn merge_converges() {
        let mut left = SyncProtocol::with_replica("a");
        let mut right = SyncProtocol::with_replica("b");
        left.checkpoint(10, 1);
        right.checkpoint(12, 2);

        let mut cloned_left = left.clone();
        let mut cloned_right = right.clone();
        cloned_left.merge(&right);
        cloned_right.merge(&left);
        assert_eq!(cloned_left.epoch, cloned_right.epoch);
        assert_eq!(cloned_left.checkpoint_lsn, cloned_right.checkpoint_lsn);
        assert_eq!(cloned_left.checkpoint_lsn, 12, "LSN es maximo");
        assert_eq!(cloned_left.epoch, 2, "epoch LWW");
        assert_eq!(cloned_left.vector_clock().len(), 2);

        // Idempotente.
        let once = cloned_left.clone();
        cloned_left.merge(&once);
        assert_eq!(cloned_left.epoch, once.epoch);
        assert_eq!(cloned_left.checkpoint_lsn, once.checkpoint_lsn);
    }
}
