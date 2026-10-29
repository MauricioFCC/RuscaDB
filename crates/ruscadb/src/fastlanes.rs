//! Bit-packing estilo FastLanes (SPEC-0176).
//!
//! Comprime `n` enteros de `b` bits sin perder informacion: `n*b` bits en vez
//! de `n*32`. Es el bloque de construccion de FOR/DELTA (ALP, Gorilla, Parquet,
//! FastLanes). El stream es LSB-first: el primer valor ocupa los bits menos
//! significativos del primer byte.
//!
//! Referencia: A. Afroozeh y P. Boncz, "The FastLanes Compression Layout",
//! PVLDB 16(11), 2023 (no re-verificado en esta sesion: PDF no accesible al
//! agente). La correccion se garantiza aqui con round-trip exhaustivo en tests.

/// Empaquetador/desempaquetador de enteros de ancho fijo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FastLanes;

impl FastLanes {
    /// Crea el codificador.
    ///
    /// Returns:
    ///     Instancia sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// Ancho de bits minimo necesario para representar `max`.
    ///
    /// Args:
    ///     max: Valor maximo del bloque.
    ///
    /// Returns:
    ///     Bits en `0..=32`.
    pub fn bit_width(max: u32) -> u32 {
        if max == 0 {
            0
        } else {
            u32::BITS - max.leading_zeros()
        }
    }

    /// Bytes necesarios para empaquetar `n` valores de `bits` bits.
    ///
    /// Args:
    ///     n: Numero de valores.
    ///     bits: Ancho en bits (`0..=32`, se satura).
    ///
    /// Returns:
    ///     Longitud del buffer empaquetado (redondeo hacia arriba).
    pub fn packed_len(n: usize, bits: u32) -> usize {
        let bits = bits.min(32) as usize;
        (n * bits).div_ceil(8)
    }

    /// Empaqueta valores truncando cada uno a `bits` bits.
    ///
    /// Args:
    ///     values: Enteros a empaquetar.
    ///     bits: Ancho por valor (`0..=32`; `0` produce un buffer vacio).
    ///
    /// Returns:
    ///     Buffer compacto con `packed_len(values.len(), bits)` bytes.
    pub fn pack(&self, values: &[u32], bits: u32) -> Vec<u8> {
        let bits = bits.min(32);
        if bits == 0 {
            return Vec::new();
        }
        let mask = if bits == 32 {
            u32::MAX
        } else {
            (1u32 << bits) - 1
        };
        let mut out = Vec::with_capacity(Self::packed_len(values.len(), bits));
        let mut acc = 0u64;
        let mut used = 0u32;
        for &value in values {
            acc |= u64::from(value & mask) << used;
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

    /// Desempaqueta `n` valores de `bits` bits.
    ///
    /// Es tolerante a buffers cortos: los valores fuera de rango se completan
    /// con `0` (un sector truncado no provoca panic, lo detecta el checksum).
    ///
    /// Args:
    ///     bytes: Buffer empaquetado.
    ///     n: Numero de valores esperados.
    ///     bits: Ancho por valor (`0..=32`, se satura).
    ///
    /// Returns:
    ///     Vector de exactamente `n` valores.
    pub fn unpack(&self, bytes: &[u8], n: usize, bits: u32) -> Vec<u32> {
        let bits = bits.min(32);
        if bits == 0 {
            return vec![0; n];
        }
        let mask = if bits == 32 {
            u32::MAX
        } else {
            (1u32 << bits) - 1
        };
        let mut out = Vec::with_capacity(n);
        let mut acc = 0u64;
        let mut used = 0u32;
        let mut cursor = 0usize;
        for _ in 0..n {
            while used < bits {
                let next = bytes.get(cursor).copied().unwrap_or(0);
                acc |= u64::from(next) << used;
                cursor += 1;
                used += 8;
            }
            out.push((acc & u64::from(mask)) as u32);
            acc >>= bits;
            used -= bits;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::FastLanes;

    /// AC-0176-01: el tamano empaquetado es el teorico y no se pierde rango.
    #[test]
    // @spec AC-0176-01
    fn test_ac_0176_01_pack() {
        let f = FastLanes::new();
        let values: Vec<u32> = (0..100).collect();
        let bits = FastLanes::bit_width(99);
        assert_eq!(bits, 7, "99 necesita 7 bits");

        let packed = f.pack(&values, bits);
        assert_eq!(
            packed.len(),
            88,
            "100 valores x 7 bits = 700 bits = 88 bytes"
        );
        assert_eq!(packed.len(), FastLanes::packed_len(values.len(), bits));

        // 4x menos que u32 crudo para este bloque.
        assert!(packed.len() * 3 < values.len() * 4);
    }

    /// AC-0176-02: round-trip exacto en todos los anchos 0..=32.
    #[test]
    // @spec AC-0176-02
    fn test_ac_0176_02_unpack() {
        let f = FastLanes::new();
        for bits in 0..=32u32 {
            let mask = if bits == 32 {
                u32::MAX
            } else if bits == 0 {
                0
            } else {
                (1u32 << bits) - 1
            };
            let values: Vec<u32> = (0..257u32)
                .map(|i| i.wrapping_mul(2_654_435_761) & mask)
                .collect();
            let packed = f.pack(&values, bits);
            let restored = f.unpack(&packed, values.len(), bits);
            assert_eq!(restored, values, "round-trip falla con bits={bits}");
        }
    }

    /// Buffer truncado: rellena con ceros, nunca panic (robustez ante torn
    /// write; la integridad la aporta CRC-32C, no el bit-packer).
    #[test]
    fn truncated_buffer_is_zero_filled() {
        let f = FastLanes::new();
        let packed = f.pack(&[1, 2, 3, 4], 16);
        let out = f.unpack(&packed[..2], 4, 16);
        assert_eq!(out, vec![1, 0, 0, 0]);
    }

    /// Los valores por encima del ancho se truncan, no corrompen vecinos.
    #[test]
    fn pack_masks_high_bits() {
        let f = FastLanes::new();
        let packed = f.pack(&[0xFFFF_FFFF, 0x0000_0001], 4);
        assert_eq!(f.unpack(&packed, 2, 4), vec![0xF, 0x1]);
    }
}
