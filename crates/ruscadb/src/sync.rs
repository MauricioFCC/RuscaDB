#![allow(missing_docs)]
#![allow(dead_code)]
//! Sync protocol (SPEC-0138): stub.

#[derive(Debug, Clone)]
pub struct SyncProtocol {
    pub checkpoint_lsn: u64,
    pub epoch: u64,
}

impl SyncProtocol {
    pub fn new() -> Self {
        Self {
            checkpoint_lsn: 0,
            epoch: 0,
        }
    }
    pub fn checkpoint(&mut self, lsn: u64, epoch: u64) {
        self.checkpoint_lsn = lsn;
        self.epoch = epoch;
    }
}

impl Default for SyncProtocol {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0138-01
    fn test_ac_0138_01_sync_checkpoint() {
        let mut sync = SyncProtocol::new();
        sync.checkpoint(42, 7);
        assert_eq!(sync.checkpoint_lsn, 42);
        assert_eq!(sync.epoch, 7);
    }
}
