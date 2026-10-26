#![allow(missing_docs)]
#![cfg(feature = "learned-index")]
#![allow(clippy::expect_used)]
use ruscadb::LearnedIndex;
#[test] // @spec AC-0174-01
fn test_ac_0174_01_build() {
    let idx = LearnedIndex::build(&[1, 2, 3, 4, 5], 2);
    assert!(!idx.segments.is_empty());
}
#[test] // @spec AC-0174-02
fn test_ac_0174_02_lookup() {
    let keys = vec![1, 2, 3];
    let idx = LearnedIndex::build(&keys, 2);
    assert_eq!(idx.lookup(&keys, 2), Some(1));
    assert_eq!(idx.lookup(&keys, 9), None);
}
