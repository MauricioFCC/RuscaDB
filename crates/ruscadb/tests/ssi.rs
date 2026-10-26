#![allow(missing_docs)]
#![cfg(feature = "ssi")]
#![allow(clippy::expect_used)]
use ruscadb::SsiTracker;
#[test] // @spec AC-0170-01
fn test_ac_0170_01_cycle() {
    let mut t = SsiTracker::new();
    t.add_rw();
    t.add_rw();
    assert!(t.has_cycle());
}
#[test] // @spec AC-0170-02
fn test_ac_0170_02_readonly() {
    let t = SsiTracker::new();
    assert!(!t.has_cycle());
}
