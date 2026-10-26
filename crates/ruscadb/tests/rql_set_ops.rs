#![allow(missing_docs)]
#![cfg(feature = "rql-set-ops")]
#![allow(clippy::expect_used)]
use ruscadb::RqlSetOps;
#[test] // @spec AC-0164-01
fn test_ac_0164_01_union() {
    let s = RqlSetOps::new();
    assert_eq!(s.union(&[1, 2], &[2, 3]), vec![1, 2, 3]);
}
#[test] // @spec AC-0164-02
fn test_ac_0164_02_union_all() {
    let s = RqlSetOps::new();
    assert_eq!(s.union_all(&[1, 2], &[2, 3]), vec![1, 2, 2, 3]);
}
#[test] // @spec AC-0164-03
fn test_ac_0164_03_intersect() {
    let s = RqlSetOps::new();
    assert_eq!(s.intersect(&[1, 2, 3], &[2, 3, 4]), vec![2, 3]);
}
#[test] // @spec AC-0164-04
fn test_ac_0164_04_except() {
    let s = RqlSetOps::new();
    assert_eq!(s.except(&[1, 2, 3], &[2]), vec![1, 3]);
}
