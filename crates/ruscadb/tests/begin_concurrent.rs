//! Tests de aceptacion de la feature `begin-concurrent` (SPEC-0131).

#![cfg(feature = "begin-concurrent")]
#![allow(clippy::expect_used)]

use ruscadb::{ConcurrentTxnManager, RuscaError};

#[test] // @spec AC-0131-01
fn test_ac_0131_01_disjoint_writers_commit() {
    let mut mgr = ConcurrentTxnManager::new();
    let tx1 = mgr.begin_concurrent(0);
    let tx2 = mgr.begin_concurrent(0);
    assert_ne!(tx1.tx_id, tx2.tx_id);

    mgr.stage_write(tx1, "fila_a").expect("stage a");
    mgr.stage_write(tx2, "fila_b").expect("stage b");
    mgr.commit(tx1).expect("commit a");
    mgr.commit(tx2).expect("commit b");
    assert!(mgr.is_committed(tx1.tx_id));
    assert!(mgr.is_committed(tx2.tx_id));
}

#[test] // @spec AC-0131-02
fn test_ac_0131_02_same_row_conflict() {
    let mut mgr = ConcurrentTxnManager::new();
    let tx1 = mgr.begin_concurrent(0);
    let tx2 = mgr.begin_concurrent(0);
    mgr.stage_write(tx1, "misma_fila").expect("stage 1");
    mgr.stage_write(tx2, "misma_fila").expect("stage 2");

    // First-committer-wins: el segundo observa el conflicto.
    mgr.commit(tx1).expect("el primero gana");
    assert!(matches!(
        mgr.commit(tx2),
        Err(RuscaError::WriteConflict { .. })
    ));
    assert!(!mgr.is_committed(tx2.tx_id));
}
