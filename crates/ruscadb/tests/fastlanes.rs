#![allow(missing_docs)]
#![cfg(feature = "fastlanes")]
#![allow(clippy::expect_used)]
use ruscadb::FastLanes;
#[test] // @spec AC-0176-01
fn test_ac_0176_01_pack() {
    let f = FastLanes::new();
    let b = f.pack(&[1, 2, 3], 4);
    assert!(!b.is_empty());
}
#[test] // @spec AC-0176-02
fn test_ac_0176_02_unpack() {
    let f = FastLanes::new();
    let b = f.pack(&[1, 2, 3], 4);
    assert_eq!(f.unpack(&b, 3), vec![1, 2, 3]);
}
