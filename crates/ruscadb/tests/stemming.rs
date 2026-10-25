//! Tests de aceptación de la feature `stemming` (SPEC-0144).

#![allow(clippy::expect_used)]

use ruscadb::SnowballStemmer;

#[test] // @spec AC-0144-01
fn test_ac_0144_01_stem_english() {
    assert_eq!(SnowballStemmer::stem("running"), "runn");
    assert_eq!(SnowballStemmer::stem("playing"), "play");
    let tokens = SnowballStemmer::tokenize("The runners are running");
    assert!(!tokens.is_empty());
}
