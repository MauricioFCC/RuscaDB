#![allow(missing_docs)]
#![cfg(feature = "rql-subqueries")]
#![allow(clippy::expect_used)]
use ruscadb::RqlSubquery;
#[test] // @spec AC-0162-01
fn test_ac_0162_01_scalar() {
    let s = RqlSubquery::new();
    assert_eq!(s.scalar(&[42, 7]), Some(42));
}
#[test] // @spec AC-0162-02
fn test_ac_0162_02_in() {
    let s = RqlSubquery::new();
    assert!(s.in_subquery(2, &[1, 2, 3]));
}
#[test] // @spec AC-0162-03
fn test_ac_0162_03_exists() {
    let s = RqlSubquery::new();
    assert!(s.exists(&[1]));
    assert!(!s.exists(&[]));
}
#[test] // @spec AC-0162-04
fn test_ac_0162_04_derived() {
    let s = RqlSubquery::new();
    assert_eq!(s.scalar(&[1, 2]).unwrap_or(0), 1);
}
