#![allow(missing_docs)]
#![cfg(feature = "page-checksums")]
#![allow(clippy::expect_used)]
use ruscadb::PageChecksum;
#[test] // @spec AC-0167-01
fn test_ac_0167_01_valid() {
    let c = PageChecksum::new();
    let d = b"hello";
    let x = c.crc32c(d);
    assert!(c.verify(d, x));
}
#[test] // @spec AC-0167-02
fn test_ac_0167_02_corrupt() {
    let c = PageChecksum::new();
    assert!(!c.verify(b"hello", 999));
}
#[test] // @spec AC-0167-03
fn test_ac_0167_03_torn() {
    let c = PageChecksum::new();
    assert!(!c.verify(b"", 1));
}
