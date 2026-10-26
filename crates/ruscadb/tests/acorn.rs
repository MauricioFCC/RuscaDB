#![allow(missing_docs)]
#![cfg(feature = "acorn")]
#![allow(clippy::expect_used)]
use ruscadb::AcornSearch;
#[test] // @spec AC-0172-01
fn test_ac_0172_01_recall() {
    let a = AcornSearch::new(4);
    assert!(a.should_expand(0.01));
}
#[test] // @spec AC-0172-02
fn test_ac_0172_02_prefilter() {
    let a = AcornSearch::new(4);
    assert!(!a.should_expand(0.5));
}
