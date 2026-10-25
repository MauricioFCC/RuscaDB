//! Tests de aceptación de la feature `join-reorder` (SPEC-0153).

#![allow(clippy::expect_used)]

use ruscadb::JoinReorderer;

#[test] // @spec AC-0153-01
fn test_ac_0153_01_join_reorder() {
    assert!(JoinReorderer::reorder(100, 10));
    assert!(!JoinReorderer::reorder(10, 100));
}
