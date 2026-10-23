//! Tests de aceptación de la feature `spatial` (SPEC-0114).

#![allow(clippy::expect_used)]

use ruscadb::RTree;

#[test] // @spec AC-0114-01
fn test_ac_0114_01_rtree_range_query() {
    let mut tree = RTree::new();
    tree.insert(1.0, 2.0);
    tree.insert(10.0, 20.0);
    let result = tree.range_query(0.0, 0.0, 5.0, 5.0);
    assert_eq!(result, vec![(1.0, 2.0)]);
}
