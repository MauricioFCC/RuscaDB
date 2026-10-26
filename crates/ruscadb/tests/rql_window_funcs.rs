#![allow(missing_docs)]
#![cfg(feature = "rql-window-funcs")]
#![allow(clippy::expect_used)]
use ruscadb::RqlWindowFuncs;
#[test] // @spec AC-0165-01
fn test_ac_0165_01_row_number() {
    let w = RqlWindowFuncs::new();
    assert_eq!(w.row_number(3), vec![1, 2, 3]);
}
#[test] // @spec AC-0165-02
fn test_ac_0165_02_sum_over() {
    let w = RqlWindowFuncs::new();
    assert_eq!(w.sum_over(&[1, 2, 3]), 6);
}
#[test] // @spec AC-0165-03
fn test_ac_0165_03_lag() {
    let w = RqlWindowFuncs::new();
    assert_eq!(w.lag(&[10, 20, 30], 1), vec![None, Some(10), Some(20)]);
}
