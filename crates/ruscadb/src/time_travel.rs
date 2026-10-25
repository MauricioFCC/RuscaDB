#![allow(missing_docs)]
#![allow(dead_code)]
//! Time-travel (SPEC-0129): lectura historica por tx_id.

/// Helper de time-travel: captura un tx_id para consultas "as of".
#[derive(Debug, Clone, Copy)]
pub struct TimeTravel {
    /// TxId del snapshot historico.
    pub tx_id: u64,
}

impl TimeTravel {
    /// Crea un snapshot historico con el tx_id dado.
    pub fn snapshot_at(tx_id: u64) -> Self {
        Self { tx_id }
    }

    /// TxId del snapshot.
    pub fn tx_id(&self) -> u64 {
        self.tx_id
    }

    /// Indica si un registro es visible en este snapshot.
    pub fn is_visible(&self, created_tx: u64, deleted_tx: Option<u64>) -> bool {
        if created_tx > self.tx_id {
            return false;
        }
        if let Some(dt) = deleted_tx
            && dt <= self.tx_id
        {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0129-01
    fn test_ac_0129_01_time_travel_snapshot() {
        let snap = TimeTravel::snapshot_at(42);
        assert_eq!(snap.tx_id(), 42);
        assert!(snap.is_visible(10, None));
        assert!(!snap.is_visible(50, None));
        assert!(!snap.is_visible(10, Some(20)));
        assert!(snap.is_visible(10, Some(50)));
    }
}
