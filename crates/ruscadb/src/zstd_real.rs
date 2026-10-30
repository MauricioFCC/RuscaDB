//! Compresion zstd (SPEC-0154): wrapper con fallback sin dependencias.
//!
//! Con la feature `zstd-compression` delega en
//! [`crate::compression::ZstdCompressor`]; sin ella, copia los bytes (identidad
//! exacta, sin perdida).

#[cfg(feature = "zstd-compression")]
pub use crate::compression::ZstdCompressor as ZstdReal;

/// Implementacion identidad cuando `zstd` no esta compilado.
#[cfg(not(feature = "zstd-compression"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct ZstdReal;

#[cfg(not(feature = "zstd-compression"))]
impl ZstdReal {
    /// Copia los bytes (fallback sin zstd).
    ///
    /// Args:
    ///     input: Bytes originales.
    ///
    /// Returns:
    ///     Los mismos bytes.
    pub fn compress(input: &[u8]) -> Vec<u8> {
        input.to_vec()
    }

    /// Copia los bytes (fallback sin zstd).
    ///
    /// Args:
    ///     input: Bytes comprimidos (identidad).
    ///
    /// Returns:
    ///     Los mismos bytes.
    pub fn decompress(input: &[u8]) -> Vec<u8> {
        input.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::ZstdReal;

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
