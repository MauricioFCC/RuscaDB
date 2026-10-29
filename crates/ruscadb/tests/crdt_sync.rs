#![allow(missing_docs)]
#![cfg(feature = "crdt-sync")]
use ruscadb::CrdtSyncReal;

#[test] // @spec AC-0179-01
fn test_ac_0179_01_merge() {
    let mut left = CrdtSyncReal::with_actor("a");
    let mut right = CrdtSyncReal::with_actor("b");
    left.set(3);
    right.set(5);
    left.merge(&right);
    right.merge(&left);
    assert_eq!(left.value(), Some(5));
    assert_eq!(left.value(), right.value());
}

#[test] // @spec AC-0179-02
fn test_ac_0179_02_delta() {
    let mut replica = CrdtSyncReal::new();
    replica.set(1);
    assert_eq!(replica.delta(), 1);
    let mut remote = CrdtSyncReal::with_actor("r");
    remote.set(2);
    replica.merge(&remote);
    assert_eq!(replica.delta(), 1);
    assert_eq!(replica.vector_clock().len(), 2);
}
