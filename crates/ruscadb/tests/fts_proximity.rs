//! Tests de aceptación de la feature `fts-proximity` (SPEC-0152).

#![allow(clippy::expect_used)]

use ruscadb::ProximityMatcher;

#[test] // @spec AC-0152-01
fn test_ac_0152_01_proximity_match() {
    assert!(ProximityMatcher::match_proximity(
        "the quick brown fox",
        "quick fox",
        1
    ));
    assert!(!ProximityMatcher::match_proximity(
        "the quick brown fox",
        "fox quick",
        1
    ));
}
