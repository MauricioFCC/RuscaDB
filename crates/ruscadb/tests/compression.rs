//! Tests de aceptación de la feature `compression` (SPEC-0115).

#![allow(clippy::expect_used)]

use ruscadb::Compressor;

#[test] // @spec AC-0115-01
fn test_ac_0115_01_compress_roundtrip() {
    let data = b"aaabbbccccdddde";
    let compressed = Compressor::compress(data);
    let decompressed = Compressor::decompress(&compressed).expect("decompress");
    assert_eq!(decompressed, data);
}
