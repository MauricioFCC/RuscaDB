//! Time-travel (SPEC-0158): visibilidad historica por `tx_id`.
//!
//! Helper de visibilidad; la via publica de consulta es
//! [`Database::execute_at_lsn`](crate::Database::execute_at_lsn) o
//! `replay_as_of`.

/// Snapshot historico por `tx_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeTravelReal {
    /// Transaccion del snapshot.
    pub tx_id: u64,
}

impl TimeTravelReal {
    /// Construye el snapshot.
    ///
    /// Args:
    ///     tx_id: Transaccion historica.
    ///
    /// Returns:
    ///     Snapshot listo.
    pub fn snapshot_at(tx_id: u64) -> Self {
        Self { tx_id }
    }

    /// Transaccion del snapshot.
    ///
    /// Returns:
    ///     `tx_id`.
    pub fn tx_id(&self) -> u64 {
        self.tx_id
    }

    /// Decide si una version es visible en este snapshot.
    ///
    /// Args:
    ///     created_tx: Transaccion de creacion.
    ///     deleted_tx: Transaccion de borrado (`None` = viva).
    ///
    /// Returns:
    ///     `true` si creada en/antes del snapshot y no borrada en/antes.
    pub fn is_visible(&self, created_tx: u64, deleted_tx: Option<u64>) -> bool {
        if created_tx > self.tx_id {
            return false;
        }
        if let Some(deleter) = deleted_tx
            && deleter <= self.tx_id
        {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::TimeTravelReal;

    #[test] // @spec AC-0158-01
    fn test_ac_0158_01_time_travel_real() {
        let snapshot = TimeTravelReal::snapshot_at(42);
        assert_eq!(snapshot.tx_id(), 42);
        assert!(snapshot.is_visible(10, None));
        assert!(!snapshot.is_visible(50, None));
        assert!(!snapshot.is_visible(10, Some(40)));
        assert!(snapshot.is_visible(10, Some(50)));
    }
}
