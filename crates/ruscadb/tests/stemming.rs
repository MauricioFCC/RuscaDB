//! Tests de aceptación de la feature `stemming` (SPEC-0144).

#![allow(clippy::expect_used)]

use ruscadb::SnowballStemmer;

#[test] // @spec AC-0144-01
fn test_ac_0144_01_stem_english() {
    // Porter 1980: step1b elimina "ing" y la consonante doble final.
    assert_eq!(SnowballStemmer::stem("running"), "run");
    // Step1c: la `y` final pasa a `i` cuando la raiz tiene vocal.
    assert_eq!(SnowballStemmer::stem("playing"), "plai");
    let tokens = SnowballStemmer::tokenize("The runners are running");
    assert!(!tokens.is_empty());
}
