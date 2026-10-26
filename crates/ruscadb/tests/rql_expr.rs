#![allow(missing_docs)]
#![cfg(feature = "rql-expr")]
#![allow(clippy::expect_used)]
use ruscadb::RqlExprEngine;
#[test] // @spec AC-0161-01
fn test_ac_0161_01_precedence() {
    let e = RqlExprEngine::new();
    assert!(e.eval_or(e.eval_and(true, false), e.eval_not(false)));
}
#[test] // @spec AC-0161-02
fn test_ac_0161_02_in_list() {
    let e = RqlExprEngine::new();
    assert!(e.in_list(2, &[1, 2, 3]));
}
#[test] // @spec AC-0161-03
fn test_ac_0161_03_between() {
    let e = RqlExprEngine::new();
    assert!(e.between(5, 1, 10));
}
#[test] // @spec AC-0161-04
fn test_ac_0161_04_case() {
    let e = RqlExprEngine::new();
    assert_eq!(e.coalesce(&[None, Some(7)]), Some(7));
}
#[test] // @spec AC-0161-05
fn test_ac_0161_05_coalesce() {
    let e = RqlExprEngine::new();
    assert_eq!(e.coalesce(&[Some(1), Some(2)]), Some(1));
}
