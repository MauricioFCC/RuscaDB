//! Tests de aceptación de la feature `zstd-real` (SPEC-0154).

#![cfg(feature = "zstd-real")]
#![allow(clippy::expect_used)]

use ruscadb::ZstdReal;

#[test] // @spec AC-0154-01
fn test_ac_0154_01_zstd_real_roundtrip() {
    let data = b"hello world";
    let compressed = ZstdReal::compress(data).expect("compress");
    let decompressed = ZstdReal::decompress(&compressed).expect("decompress");
    assert_eq!(decompressed, data);
}
