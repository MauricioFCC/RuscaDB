#![allow(missing_docs)]
#![cfg(feature = "persistent-hnsw")]
#![allow(clippy::expect_used)]
use ruscadb::PersistentHnsw;
#[test] // @spec AC-0173-01
fn test_ac_0173_01_roundtrip() {
    let mut h = PersistentHnsw::new();
    h.add();
    h.add();
    let b = h.save();
    let h2 = PersistentHnsw::load(&b).unwrap();
    assert_eq!(h2.vectors, 2);
}
#[test] // @spec AC-0173-02
fn test_ac_0173_02_corrupt() {
    assert!(PersistentHnsw::load(b"garbage").is_none());
}
