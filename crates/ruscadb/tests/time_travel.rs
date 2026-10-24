//! Tests de aceptación de la feature `time-travel` (SPEC-0129).

#![allow(clippy::expect_used)]

use ruscadb::TimeTravel;

#[test] // @spec AC-0129-01
fn test_ac_0129_01_time_travel_snapshot() {
    let snap = TimeTravel::snapshot_at(42);
    assert_eq!(snap.tx_id(), 42);
    assert!(snap.is_visible(10, None));
    assert!(!snap.is_visible(50, None));
    assert!(!snap.is_visible(10, Some(20)));
    assert!(snap.is_visible(10, Some(50)));
}
