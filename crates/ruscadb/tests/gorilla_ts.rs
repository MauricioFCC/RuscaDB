#![allow(missing_docs)]
#![cfg(feature = "gorilla-ts")]
#![allow(clippy::expect_used)]
use ruscadb::GorillaTs;
#[test] // @spec AC-0178-01
fn test_ac_0178_01_ratio() {
    let g = GorillaTs::new();
    let v = vec![1.0, 2.0, 3.0];
    let b = g.encode(&v);
    assert!(!b.is_empty());
}
#[test] // @spec AC-0178-02
fn test_ac_0178_02_lossless() {
    let g = GorillaTs::new();
    let v = vec![1.0, 2.0, 3.0];
    let b = g.encode(&v);
    assert_eq!(g.decode(&b), v);
}
