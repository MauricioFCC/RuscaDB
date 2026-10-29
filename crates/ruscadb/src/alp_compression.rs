//! Compresion sin perdida de `f64` estilo ALP (SPEC-0175).
//!
//! ALP (SIGMOD 2024) explota que los `f64` reales suelen ser decimales con
//! pocos digitos: busca un exponente decimal `e` que convierta el vector en
//! enteros exactos y despues aplica FOR (frame of reference) + bit-packing.
//! Los valores que no son exactos se guardan como **excepciones** (indice +
//! bits originales), y si las excepciones dominan se usa un modo crudo.
//!
//! La exactitud es un invariante comprobado: solo se elige `e` para un valor si
//! `v*10^e` es integral y la division inversa reproduce el `f64` original bit a
//! bit. El test de round-trip lo verifica sobre valores hostiles (0.1, PI, NaN).
//!
//! Referencia: A. Afroozeh et al., "ALP: Adaptive Lossless floating-Point
//! Compression", SIGMOD 2024 (no re-verificado en esta sesion: PDF no
//! accesible al agente); el invariante de exactitud se valida con tests.

/// Cabecera magica del formato ALP de RuscaDB.
const MAGIC: u8 = 0xA1;

/// Modo: exponente decimal + FOR + bit-packing (+ excepciones).
const MODE_ALP: u8 = 0;
/// Modo: `f64` crudo bit-exacto (fallback).
const MODE_RAW: u8 = 1;

/// Cota de seguridad para que el entero escalado quepa en un `i64` holgado.
const MAX_SCALED: f64 = 274_877_906_944.0;

/// Compresor ALP de `f64`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AlpCompressor;

/// Escribe `n` bits LSB-first de cada valor.
///
/// Args:
///     values: Valores ya normalizados (delta sobre el minimo).
///     bits: Ancho en bits (`0..=64`).
///
/// Returns:
///     Buffer empaquetado.
fn pack_bits(values: &[u64], bits: u32) -> Vec<u8> {
    if bits == 0 || values.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity((values.len() * bits as usize).div_ceil(8));
    let mut acc = 0u128;
    let mut used = 0u32;
    for &value in values {
        acc |= u128::from(value) << used;
        used += bits;
        while used >= 8 {
            out.push((acc & 0xFF) as u8);
            acc >>= 8;
            used -= 8;
        }
    }
    if used > 0 {
        out.push((acc & 0xFF) as u8);
    }
    out
}

/// Lee `n` valores de `bits` bits LSB-first.
///
/// Args:
///     bytes: Buffer empaquetado.
///     n: Numero de valores.
///     bits: Ancho en bits (`0..=64`).
///
/// Returns:
///     Valores leidos (cero si el buffer se agota).
fn unpack_bits(bytes: &[u8], n: usize, bits: u32) -> Vec<u64> {
    if bits == 0 {
        return vec![0; n];
    }
    let mask = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let mut out = Vec::with_capacity(n);
    let mut acc = 0u128;
    let mut used = 0u32;
    let mut cursor = 0usize;
    for _ in 0..n {
        while used < bits {
            let byte = bytes.get(cursor).copied().unwrap_or(0);
            acc |= u128::from(byte) << used;
            cursor += 1;
            used += 8;
        }
        out.push((acc as u64) & mask);
        acc >>= bits;
        used -= bits;
    }
    out
}

/// Longitud del bloque bit-packed para `n` valores de `bits` bits.
fn packed_len(n: usize, bits: u32) -> usize {
    (n * bits as usize).div_ceil(8)
}

impl AlpCompressor {
    /// Crea el compresor.
    ///
    /// Returns:
    ///     Instancia sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// Comprueba si `value` se puede escalar a entero exacto con `e`.
    ///
    /// Args:
    ///     value: Valor a comprobar.
    ///     scale: `10^e`.
    ///
    /// Returns:
    ///     `true` si la transformacion es reversible bit a bit.
    fn is_encodable(value: f64, scale: f64) -> bool {
        if !value.is_finite() {
            return false;
        }
        let scaled = value * scale;
        if !scaled.is_finite() || scaled.abs() >= MAX_SCALED || scaled.fract() != 0.0 {
            return false;
        }
        (scaled as i64) as f64 / scale == value
            && ((scaled as i64) as f64 / scale).to_bits() == value.to_bits()
    }

    /// Busca el exponente decimal que maximiza los valores codificables.
    ///
    /// Args:
    ///     values: Vector de entrada.
    ///
    /// Returns:
    ///     `Some(e)` si al menos la mitad de los valores son codificables;
    ///     `None` si conviene el modo crudo.
    fn detect_exponent(values: &[f64]) -> Option<u8> {
        if values.is_empty() {
            return Some(0);
        }
        let mut best: Option<(u8, usize)> = None;
        for e in 0u8..=18 {
            let scale = 10f64.powi(i32::from(e));
            let count = values
                .iter()
                .filter(|&&value| Self::is_encodable(value, scale))
                .count();
            if count * 2 < values.len() {
                continue;
            }
            match best {
                Some((_, best_count)) if best_count >= count => {}
                _ => best = Some((e, count)),
            }
        }
        best.map(|(e, _)| e)
    }

    /// Comprime un vector de `f64`.
    ///
    /// Formato: `magic | mode | e | n(u32 LE)` y luego `min(i64) | width(u8) |
    /// bits | exc_count(u32) | {index(u32), bits(u64)}*` (ALP) o `n * f64 LE`
    /// (crudo).
    ///
    /// Args:
    ///     values: Vector a comprimir.
    ///
    /// Returns:
    ///     Buffer comprimido que [`Self::decode`] reconstruye bit a bit.
    pub fn encode(&self, values: &[f64]) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + values.len() * 8);
        out.push(MAGIC);
        let n = values.len() as u32;

        let Some(e) = Self::detect_exponent(values) else {
            out.push(MODE_RAW);
            out.push(0);
            out.extend_from_slice(&n.to_le_bytes());
            for &value in values {
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            return out;
        };

        let scale = 10f64.powi(i32::from(e));
        let mut ints = vec![0i64; values.len()];
        let mut encodable = vec![false; values.len()];
        let mut exceptions: Vec<(u32, u64)> = Vec::new();
        for (index, &value) in values.iter().enumerate() {
            if Self::is_encodable(value, scale) {
                ints[index] = (value * scale) as i64;
                encodable[index] = true;
            } else {
                exceptions.push((index as u32, value.to_bits()));
            }
        }
        let min = ints
            .iter()
            .zip(&encodable)
            .filter(|(_, ok)| **ok)
            .map(|(&value, _)| value)
            .min()
            .unwrap_or(0);
        let deltas: Vec<u64> = ints
            .iter()
            .zip(&encodable)
            .map(|(&value, ok)| if *ok { (value - min) as u64 } else { 0 })
            .collect();
        let max_delta = deltas
            .iter()
            .zip(&encodable)
            .filter(|(_, ok)| **ok)
            .map(|(&delta, _)| delta)
            .max()
            .unwrap_or(0);
        let width = if max_delta == 0 {
            0
        } else {
            64 - max_delta.leading_zeros()
        };

        out.push(MODE_ALP);
        out.push(e);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&min.to_le_bytes());
        out.push(width as u8);
        out.extend_from_slice(&pack_bits(&deltas, width));
        out.extend_from_slice(&(exceptions.len() as u32).to_le_bytes());
        for (index, bits) in exceptions {
            out.extend_from_slice(&index.to_le_bytes());
            out.extend_from_slice(&bits.to_le_bytes());
        }
        out
    }

    /// Descomprime un buffer producido por [`Self::encode`].
    ///
    /// Args:
    ///     bytes: Buffer comprimido.
    ///
    /// Returns:
    ///     Vector reconstruido. Un buffer invalido o truncado devuelve vacio
    ///     en vez de hacer panic.
    pub fn decode(&self, bytes: &[u8]) -> Vec<f64> {
        if bytes.len() < 7 || bytes[0] != MAGIC {
            return Vec::new();
        }
        let mode = bytes[1];
        let e = bytes[2];
        let n = u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]) as usize;
        if n == 0 {
            return Vec::new();
        }
        match mode {
            MODE_ALP => {
                if bytes.len() < 16 {
                    return Vec::new();
                }
                let min = i64::from_le_bytes([
                    bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13],
                    bytes[14],
                ]);
                let width = u32::from(bytes[15]).min(64);
                let block_len = packed_len(n, width);
                if bytes.len() < 16 + block_len + 4 {
                    return Vec::new();
                }
                let deltas = unpack_bits(&bytes[16..16 + block_len], n, width);
                let scale = 10f64.powi(i32::from(e));
                let mut out: Vec<f64> = deltas
                    .into_iter()
                    .map(|delta| min.wrapping_add(delta as i64) as f64 / scale)
                    .collect();

                let exception_offset = 16 + block_len;
                let exception_count = u32::from_le_bytes([
                    bytes[exception_offset],
                    bytes[exception_offset + 1],
                    bytes[exception_offset + 2],
                    bytes[exception_offset + 3],
                ]) as usize;
                let mut cursor = exception_offset + 4;
                for _ in 0..exception_count {
                    if cursor + 12 > bytes.len() {
                        return Vec::new();
                    }
                    let index = u32::from_le_bytes([
                        bytes[cursor],
                        bytes[cursor + 1],
                        bytes[cursor + 2],
                        bytes[cursor + 3],
                    ]) as usize;
                    let bits = u64::from_le_bytes([
                        bytes[cursor + 4],
                        bytes[cursor + 5],
                        bytes[cursor + 6],
                        bytes[cursor + 7],
                        bytes[cursor + 8],
                        bytes[cursor + 9],
                        bytes[cursor + 10],
                        bytes[cursor + 11],
                    ]);
                    if let Some(slot) = out.get_mut(index) {
                        *slot = f64::from_bits(bits);
                    }
                    cursor += 12;
                }
                out
            }
            MODE_RAW => {
                if bytes.len() < 7 + n * 8 {
                    return Vec::new();
                }
                bytes[7..7 + n * 8]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| f64::from_bits(u64::from_le_bytes(*c)))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Indica si un vector se codifico en modo decimal o en crudo.
    ///
    /// Args:
    ///     bytes: Buffer producido por [`Self::encode`].
    ///
    /// Returns:
    ///     `true` si uso ALP decimal (false = crudo).
    pub fn is_decimal_encoded(&self, bytes: &[u8]) -> bool {
        bytes.len() > 1 && bytes[0] == MAGIC && bytes[1] == MODE_ALP
    }
}

#[cfg(test)]
mod tests {
    use super::AlpCompressor;

    /// AC-0175-01: decimales de 2 digitos baten holgadamente a `f64` crudo.
    #[test]
    // @spec AC-0175-01
    fn test_ac_0175_01_ratio() {
        let alp = AlpCompressor::new();
        let values: Vec<f64> = (0..1000).map(|i| 10.0 + f64::from(i % 50) * 0.25).collect();
        let packed = alp.encode(&values);
        assert!(alp.is_decimal_encoded(&packed), "deberia usar modo ALP");
        assert!(
            packed.len() < values.len() * 2,
            "ALP debe batir 2 bytes/valor: {} bytes para {} valores",
            packed.len(),
            values.len()
        );
        assert!(packed.len() < values.len() * 8 / 4, "al menos 4x vs crudo");
    }

    /// AC-0175-02: round-trip bit-exacto, incluidos los que caen a excepcion.
    #[test]
    // @spec AC-0175-02
    fn test_ac_0175_02_lossless() {
        let alp = AlpCompressor::new();
        let cases: Vec<Vec<f64>> = vec![
            vec![1.25, 4.5, -7.75],
            vec![1.0, 2.0, 3.0, 4.0],
            vec![0.0, -0.0, 1e-9],
            vec![std::f64::consts::PI, std::f64::consts::E],
            vec![f64::NAN, f64::INFINITY, 1.5],
            vec![0.1, 0.2, 0.3],
            vec![1.75, 0.07, 2.5, 0.07],
            Vec::new(),
        ];
        for values in cases {
            let packed = alp.encode(&values);
            let restored = alp.decode(&packed);
            let original: Vec<u64> = values.iter().map(|v| v.to_bits()).collect();
            let decoded: Vec<u64> = restored.iter().map(|v| v.to_bits()).collect();
            assert_eq!(original, decoded, "round-trip exacto para {values:?}");
        }
    }

    /// Fallback: valores no decimales usan modo crudo pero siguen siendo exactos.
    #[test]
    fn non_decimal_falls_back_to_raw() {
        let alp = AlpCompressor::new();
        let values = vec![std::f64::consts::PI, std::f64::consts::SQRT_2];
        let packed = alp.encode(&values);
        assert!(!alp.is_decimal_encoded(&packed));
        assert_eq!(
            alp.decode(&packed)
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            values.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }

    /// Buffer truncado o corrupto no hace panic.
    #[test]
    fn invalid_buffers_are_rejected() {
        let alp = AlpCompressor::new();
        assert!(alp.decode(&[]).is_empty());
        assert!(alp.decode(&[0xFF; 8]).is_empty());
        let packed = alp.encode(&[1.25, 2.5]);
        for cut in 0..packed.len() {
            let _ = alp.decode(&packed[..cut]);
        }
    }
}
