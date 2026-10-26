#![allow(missing_docs)]
#![cfg(feature = "crdt-sync")]
#![allow(clippy::expect_used)]
use ruscadb::CrdtSyncReal;
#[test] // @spec AC-0179-01
fn test_ac_0179_01_merge() {
    let c = CrdtSyncReal::new();
    assert_eq!(c.merge(3, 5), 5);
}
#[test] // @spec AC-0179-02
fn test_ac_0179_02_delta() {
    let mut c = CrdtSyncReal::new();
    c.tick();
    assert_eq!(c.delta(), 1);
}
