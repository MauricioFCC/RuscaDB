//! Tests de aceptación de la feature `fts-advanced` (SPEC-0117).

#![allow(clippy::expect_used)]

use ruscadb::{FuzzyMatcher, PhraseMatcher};

#[test] // @spec AC-0117-01
fn test_ac_0117_01_levenshtein() {
    assert_eq!(FuzzyMatcher::levenshtein("kitten", "sitting"), 3);
    assert!(FuzzyMatcher::is_fuzzy_match("hello", "hallo", 1));
}

#[test] // @spec AC-0117-02
fn test_ac_0117_02_phrase_match() {
    assert!(PhraseMatcher::match_phrase(
        "the quick brown fox",
        "quick fox"
    ));
    assert!(!PhraseMatcher::match_phrase(
        "the quick brown fox",
        "fox quick"
    ));
}
