//! Tests de aceptación de la feature `time-travel-real` (SPEC-0158).

#![allow(clippy::expect_used)]

use ruscadb::TimeTravelReal;

#[test] // @spec AC-0158-01
fn test_ac_0158_01_time_travel_real() {
    let snap = TimeTravelReal::snapshot_at(42);
    assert_eq!(snap.tx_id(), 42);
    assert!(snap.is_visible(10, None));
    assert!(!snap.is_visible(50, None));
}
