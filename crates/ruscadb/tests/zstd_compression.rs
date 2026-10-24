//! Tests de aceptación de la feature `zstd-compression` (SPEC-0121).

#![allow(clippy::expect_used)]

#[cfg(feature = "zstd-compression")]
use ruscadb::ZstdCompressor;

#[cfg(feature = "zstd-compression")]
#[test] // @spec AC-0121-01
fn test_ac_0121_01_zstd_roundtrip() {
    let data = b"aaabbbccccddddeeeee";
    let compressed = ZstdCompressor::compress(data).expect("compress");
    let decompressed = ZstdCompressor::decompress(&compressed).expect("decompress");
    assert_eq!(decompressed, data);
}

#[cfg(feature = "zstd-compression")]
#[test] // @spec AC-0121-02
fn test_ac_0121_02_zstd_empty() {
    let compressed = ZstdCompressor::compress(&[]).expect("compress empty");
    let decompressed = ZstdCompressor::decompress(&compressed).expect("decompress empty");
    assert!(decompressed.is_empty());
}
