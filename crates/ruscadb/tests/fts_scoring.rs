//! Tests de aceptación de la feature `fts-scoring` (SPEC-0123).

#![allow(clippy::expect_used)]

use ruscadb::FuzzyScorer;

#[test] // @spec AC-0123-01
fn test_ac_0123_01_fuzzy_score() {
    let exact = FuzzyScorer::score(1.0, "hello", "hello", 0.5);
    let fuzzy = FuzzyScorer::score(1.0, "hello", "hallo", 0.5);
    assert!(exact > fuzzy, "documento exacto debe tener score mayor");
}
