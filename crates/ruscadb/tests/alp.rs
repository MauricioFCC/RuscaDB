#![allow(missing_docs)]
#![cfg(feature = "alp")]
#![allow(clippy::expect_used)]
use ruscadb::AlpCompressor;
#[test] // @spec AC-0175-01
fn test_ac_0175_01_ratio() {
    let c = AlpCompressor::new();
    let v = vec![1.23, 4.56];
    let b = c.encode(&v);
    assert!(!b.is_empty());
}
#[test] // @spec AC-0175-02
fn test_ac_0175_02_lossless() {
    let c = AlpCompressor::new();
    let v = vec![1.23, 4.56];
    let b = c.encode(&v);
    assert_eq!(c.decode(&b), v);
}
