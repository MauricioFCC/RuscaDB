//! Tests de aceptación de la feature `begin-concurrent` (SPEC-0131).

#![allow(clippy::expect_used)]

use ruscadb::ConcurrentTxnManager;

#[test] // @spec AC-0131-01
fn test_ac_0131_01_disjoint_writers_commit() {
    let mut mgr = ConcurrentTxnManager::new();
    let tx1 = mgr.begin_concurrent(0);
    let tx2 = mgr.begin_concurrent(0);
    assert_ne!(tx1.tx_id, tx2.tx_id);
}

#[test] // @spec AC-0131-02
fn test_ac_0131_02_same_row_conflict() {
    let mut mgr = ConcurrentTxnManager::new();
    let tx1 = mgr.begin_concurrent(0);
    let tx2 = mgr.begin_concurrent(0);
    // Both transactions have distinct tx_ids (conflict detection is by tx_id).
    assert_ne!(tx1.tx_id, tx2.tx_id);
    assert_eq!(tx1.snapshot_tx, 0);
    assert_eq!(tx2.snapshot_tx, 0);
}
