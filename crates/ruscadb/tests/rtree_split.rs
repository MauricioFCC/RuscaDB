//! Tests de aceptación de la feature `rtree-split` (SPEC-0156).

#![allow(clippy::expect_used)]

use ruscadb::RTreeSplit;

#[test] // @spec AC-0156-01
fn test_ac_0156_01_rtree_split_range() {
    let mut tree = RTreeSplit::new();
    tree.insert(1.0, 2.0);
    tree.insert(10.0, 20.0);
    let result = tree.range_query(0.0, 0.0, 5.0, 5.0);
    assert_eq!(result, vec![(1.0, 2.0)]);
}
