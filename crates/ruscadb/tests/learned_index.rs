#![allow(missing_docs)]
#![cfg(feature = "learned-index")]
use ruscadb::LearnedIndex;

#[test] // @spec AC-0174-01
fn test_ac_0174_01_build() {
    let keys: Vec<i64> = (0..300).map(|i| i * i).collect();
    let index = LearnedIndex::build(&keys, 4);
    assert!(!index.segments.is_empty());
    for (position, &key) in keys.iter().enumerate() {
        let predicted = index.predict(key).unwrap_or(usize::MAX);
        assert!(predicted.abs_diff(position) <= index.epsilon);
    }
}

#[test] // @spec AC-0174-02
fn test_ac_0174_02_lookup() {
    let keys: Vec<i64> = (0..200).map(|i| 1_000 + i * 7).collect();
    let index = LearnedIndex::build(&keys, 8);
    for (position, &key) in keys.iter().enumerate() {
        assert_eq!(index.lookup(key), Some(position));
    }
    assert_eq!(index.lookup(999), None);
    assert_eq!(index.lookup(i64::MAX), None);
}
