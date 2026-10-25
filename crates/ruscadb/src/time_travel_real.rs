#![allow(missing_docs)]
#![allow(dead_code)]
//! Time-travel real (SPEC-0158): snapshot por tx_id.

#[derive(Debug, Clone, Copy)]
pub struct TimeTravelReal {
    pub tx_id: u64,
}

impl TimeTravelReal {
    pub fn snapshot_at(tx_id: u64) -> Self {
        Self { tx_id }
    }
    pub fn tx_id(&self) -> u64 {
        self.tx_id
    }
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
    #[test] // @spec AC-0158-01
    fn test_ac_0158_01_time_travel_real() {
        let snap = TimeTravelReal::snapshot_at(42);
        assert_eq!(snap.tx_id(), 42);
        assert!(snap.is_visible(10, None));
        assert!(!snap.is_visible(50, None));
    }
}
