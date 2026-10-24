//! Tests de aceptación de la feature `spatial` (SPEC-0114).

#![allow(clippy::expect_used)]

use ruscadb::{Point, RTree};

#[test] // @spec AC-0114-01
fn test_ac_0114_01_rtree_range_query() {
    let mut tree = RTree::new();
    tree.insert_xy(1.0, 2.0);
    tree.insert_xy(10.0, 20.0);
    let result = tree.range_query_xy(0.0, 0.0, 5.0, 5.0);
    assert_eq!(result, vec![Point::new(1.0, 2.0)]);
}

#[test]
fn test_rtree_knn() {
    let mut tree = RTree::new();
    tree.insert(Point::new(0.0, 0.0));
    tree.insert(Point::new(3.0, 4.0));
    tree.insert(Point::new(10.0, 10.0));
    let nearest = tree.knn(&Point::new(0.0, 0.0), 2);
    assert_eq!(nearest.len(), 2);
    assert_eq!(nearest[0], Point::new(0.0, 0.0));
}
