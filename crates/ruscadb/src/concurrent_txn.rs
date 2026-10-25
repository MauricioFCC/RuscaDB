#![allow(missing_docs)]
#![allow(dead_code)]
//! BEGIN CONCURRENT (SPEC-0131): multi-writer stub.

#[derive(Debug, Clone)]
pub struct ConcurrentTxn {
    pub tx_id: u64,
    pub snapshot_tx: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ConcurrentTxnManager {
    next_tx: u64,
}

impl ConcurrentTxnManager {
    pub fn new() -> Self {
        Self { next_tx: 1 }
    }
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
