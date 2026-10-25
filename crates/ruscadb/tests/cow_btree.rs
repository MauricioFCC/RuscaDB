//! Tests de aceptación de la feature `cow-btree` (SPEC-0130).

#![allow(clippy::expect_used)]

use ruscadb::CowBTree;

#[test] // @spec AC-0130-01
fn test_ac_0130_01_cow_snapshot_isolation() {
    let mut tree = CowBTree::new();
    tree.insert(1, "a".to_string());
    assert_eq!(tree.get(1), Some("a".to_string()));
    assert_eq!(tree.epoch(), 1);
    tree.insert(2, "b".to_string());
    assert_eq!(tree.len(), 2);
}

#[test] // @spec AC-0130-02
fn test_ac_0130_02_cow_reclaim_after_checkpoint() {
    let mut tree = CowBTree::new();
    tree.insert(1, "a".to_string());
    tree.insert(2, "b".to_string());
    assert_eq!(tree.len(), 2);
    assert_eq!(tree.epoch(), 2);
    let empty = CowBTree::new();
    assert_eq!(empty.epoch(), 0);
    assert!(empty.is_empty());
}
