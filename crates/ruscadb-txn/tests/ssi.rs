//! Cableado de SSI en `TxnManager` (SPEC-0170).
//!
//! Comprueba que las aristas rw se registran al confirmar (writer -> lectores
//! concurrentes) y que una estructura peligrosa aborta el commit del pivote.

#![allow(missing_docs)]

use ruscadb_core::RuscaError;
use ruscadb_txn::{IsoLevel, TxnManager};

/// El pivote `T1 -rw-> T2 -rw-> T3` aborta al confirmar `T2`.
#[test]
fn ssi_aborts_dangerous_structure_on_commit() {
    let mut txn = TxnManager::new();
    txn.enable_ssi(true);
    assert!(txn.is_ssi_enabled());

    let t1 = txn.begin();
    let t2 = txn.begin();
    let t3 = txn.begin();

    // T1 escribe k; T2 lo lee antes del commit de T1 -> T1 -rw-> T2.
    txn.stage_write(t1, "k".to_string()).expect("stage t1");
    txn.record_read(t2, "k");
    txn.commit(t1).expect("t1 sin conflicto");

    // T2 escribe m; T3 lo lee -> T2 -rw-> T3 (T2 es pivote).
    txn.stage_write(t2, "m".to_string()).expect("stage t2");
    txn.record_read(t3, "m");
    assert!(
        matches!(txn.commit(t2), Err(RuscaError::WriteConflict { .. })),
        "el pivote debe abortar"
    );
    // T3 no participa en un ciclo: puede retirarse limpiamente.
    txn.rollback(t3).expect("rollback t3");
}

/// Sin SSI, el mismo historial no aborta (comportamiento previo intacto).
#[test]
fn ssi_is_opt_in() {
    let mut txn = TxnManager::new();
    let t1 = txn.begin();
    let t2 = txn.begin();
    let t3 = txn.begin();
    txn.stage_write(t1, "k".to_string()).expect("stage");
    txn.record_read(t2, "k");
    txn.commit(t1).expect("t1");
    txn.stage_write(t2, "m".to_string()).expect("stage");
    txn.record_read(t3, "m");
    txn.commit(t2).expect("sin SSI no hay abort");
    txn.commit(t3).expect("t3");
}

/// Las aristas rw quedan visibles en el tracker y el pivote se detecta.
#[test]
fn ssi_tracker_records_edges() {
    let mut txn = TxnManager::new();
    txn.enable_ssi(true);
    let reader = txn.begin();
    let writer = txn.begin();
    txn.record_read(reader, "x");
    txn.stage_write(writer, "x".to_string()).expect("stage");
    txn.commit(writer).expect("writer");
    assert_eq!(txn.ssi_tracker().edge_count(), 1);
    txn.rollback(reader).expect("reader");
    assert_eq!(txn.ssi_tracker().edge_count(), 0, "rollback retira aristas");
}

/// Los niveles de aislamiento siguen expuestos desde la fachada.
#[test]
fn iso_levels_are_exported() {
    assert_ne!(IsoLevel::ReadCommitted, IsoLevel::Snapshot);
    assert_ne!(IsoLevel::Snapshot, IsoLevel::Serializable);
}
