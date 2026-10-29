#![allow(missing_docs)]
#![cfg(feature = "page-checksums")]
use ruscadb::PageChecksum;

#[test] // @spec AC-0167-01
fn test_ac_0167_01_valid() {
    let checksum = PageChecksum::new();
    assert_eq!(checksum.crc32c(b"123456789"), 0xE306_9283);
    let mut page = [0u8; 4096];
    assert!(checksum.stamp(&mut page));
    assert!(checksum.verify_stamped(&page));
}

#[test] // @spec AC-0167-02
fn test_ac_0167_02_corrupt() {
    let checksum = PageChecksum::new();
    let mut page = vec![9u8; 256];
    assert!(checksum.stamp(&mut page));
    let good = page.clone();
    page[17] ^= 0b0000_1000;
    assert!(!checksum.verify_stamped(&page));
    assert!(checksum.verify_stamped(&good));
}

#[test] // @spec AC-0167-03
fn test_ac_0167_03_torn() {
    let checksum = PageChecksum::new();
    assert!(!checksum.verify_stamped(&[]));
    assert!(!checksum.verify_stamped(b"abc"));
    let mut buffer = [0u8; 2];
    assert!(!checksum.stamp(&mut buffer));
}
