//! Compresion de WAL opcional (SPEC-0115): RLE sin dependencias externas.

/// Compresor RLE (run-length encoding) sin dependencias.
#[cfg(feature = "compression")]
#[derive(Debug, Clone, Copy, Default)]
pub struct Compressor;

#[cfg(feature = "compression")]
impl Compressor {
    /// Comprime un slice de bytes con RLE.
    pub fn compress(input: &[u8]) -> Vec<u8> {
        if input.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut current = input[0];
        let mut count: u8 = 1;
        for &byte in &input[1..] {
            if byte == current && count < 255 {
                count += 1;
            } else {
                out.push(count);
                out.push(current);
                current = byte;
                count = 1;
            }
        }
        out.push(count);
        out.push(current);
        out
    }

    /// Descomprime un slice RLE.
    pub fn decompress(input: &[u8]) -> Result<Vec<u8>, String> {
        if input.is_empty() {
            return Ok(Vec::new());
        }
        if !input.len().is_multiple_of(2) {
            return Err("entrada RLE invalida: longitud impar".to_string());
        }
        let mut out = Vec::new();
        for chunk in input.chunks(2) {
            let count = chunk[0] as usize;
            let byte = chunk[1];
            for _ in 0..count {
                out.push(byte);
            }
        }
        Ok(out)
    }
}

/// Compresor zstd (SPEC-0121, feature `zstd-compression`).
#[cfg(feature = "zstd-compression")]
pub struct ZstdCompressor;

#[cfg(feature = "zstd-compression")]
impl ZstdCompressor {
    /// Comprime un slice de bytes con zstd (nivel 3).
    pub fn compress(input: &[u8]) -> Result<Vec<u8>, String> {
        zstd::encode_all(input, 3).map_err(|e| format!("zstd compress: {e}"))
    }

    /// Descomprime un slice zstd.
    pub fn decompress(input: &[u8]) -> Result<Vec<u8>, String> {
        zstd::decode_all(input).map_err(|e| format!("zstd decompress: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0115-01
    fn test_ac_0115_01_compress_roundtrip() {
        let data = b"aaabbbccccdddde";
        let compressed = Compressor::compress(data);
        let decompressed = Compressor::decompress(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_empty() {
        assert!(Compressor::compress(&[]).is_empty());
        assert!(Compressor::decompress(&[]).unwrap().is_empty());
    }
}
