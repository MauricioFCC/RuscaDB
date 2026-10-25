#![allow(missing_docs)]
#![allow(dead_code)]
//! Compresion zstd real (SPEC-0154): wrapper sobre zstd crate.

#[cfg(feature = "zstd-compression")]
pub use crate::compression::ZstdCompressor as ZstdReal;

#[cfg(not(feature = "zstd-compression"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct ZstdReal;

#[cfg(not(feature = "zstd-compression"))]
impl ZstdReal {
    pub fn compress(input: &[u8]) -> Vec<u8> {
        input.to_vec()
    }
    pub fn decompress(input: &[u8]) -> Vec<u8> {
        input.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0154-01
    fn test_ac_0154_01_zstd_real_roundtrip() {
        let data = b"hello world";
        #[cfg(feature = "zstd-compression")]
        {
            let compressed = ZstdReal::compress(data).expect("compress");
            let decompressed = ZstdReal::decompress(&compressed).expect("decompress");
            assert_eq!(decompressed, data);
        }
        #[cfg(not(feature = "zstd-compression"))]
        {
            let compressed = ZstdReal::compress(data);
            let decompressed = ZstdReal::decompress(&compressed);
            assert_eq!(decompressed, data);
        }
    }
}
