#![allow(missing_docs)]
#![cfg(feature = "ssi")]
use ruscadb::SsiTracker;

#[test] // @spec AC-0170-01
fn test_ac_0170_01_cycle() {
    let mut tracker = SsiTracker::new();
    tracker.add_rw_edge(1, 2);
    tracker.add_rw_edge(2, 1);
    assert!(tracker.has_cycle());
    assert!(tracker.should_abort(1));
    assert!(!tracker.is_serializable());
}

#[test] // @spec AC-0170-02
fn test_ac_0170_02_readonly() {
    let mut tracker = SsiTracker::new();
    tracker.register(1);
    tracker.register(2);
    assert!(!tracker.has_cycle());
    assert!(!tracker.has_dangerous_structure());
    assert!(tracker.is_serializable());
    assert!(!tracker.should_abort(1));
}
