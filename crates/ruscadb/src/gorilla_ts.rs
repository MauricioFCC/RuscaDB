//! Compresion de series temporales con XOR de Gorilla (SPEC-0178).
//!
//! Algoritmo de Facebook Gorilla (VLDB 2015): codifica cada `f64` como XOR con
//! el anterior y reutiliza la ventana de bits significativos mientras se
//! mantenga, lo que da 1 bit por valor en series constantes. La implementacion
//! es bit-exacta: preserva el patron binario de `-0.0`, `NaN` con payload e
//! infinitos (se compara con `to_bits`, no con `==`).

/// Lector/escritor de bits LSB-first.
#[derive(Debug, Default)]
struct BitStream {
    /// Bytes ya cerrados.
    bytes: Vec<u8>,
    /// Acumulador de bits (u128 para admitir una escritura de 64 bits con
    /// hasta 7 bits pendientes sin perder informacion).
    acc: u128,
    /// Bits ocupados en `acc`.
    used: u32,
}

impl BitStream {
    /// Crea un stream vacio.
    fn new() -> Self {
        Self::default()
    }

    /// Escribe los `n` bits menos significativos de `value`.
    ///
    /// Args:
    ///     value: Bits a escribir.
    ///     n: Numero de bits (`0..=64`).
    fn write(&mut self, value: u64, n: u32) {
        debug_assert!(n <= 64);
        self.acc |= u128::from(value) << self.used;
        self.used += n;
        while self.used >= 8 {
            self.bytes.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.used -= 8;
        }
    }

    /// Cierra el stream volcando el byte parcial.
    ///
    /// Returns:
    ///     Bytes completos.
    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.bytes.push((self.acc & 0xFF) as u8);
        }
        self.bytes
    }
}

/// Lector de bits LSB-first con relleno de ceros al agotarse.
#[derive(Debug)]
struct BitReader<'a> {
    /// Bytes de entrada.
    bytes: &'a [u8],
    /// Posicion del proximo byte a ingerir.
    cursor: usize,
    /// Acumulador.
    acc: u128,
    /// Bits validos en `acc`.
    used: u32,
}

impl<'a> BitReader<'a> {
    /// Envuelve un buffer.
    ///
    /// Args:
    ///     bytes: Stream de bits.
    ///
    /// Returns:
    ///     Lector en la posicion 0.
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            cursor: 0,
            acc: 0,
            used: 0,
        }
    }

    /// Lee `n` bits (los que falten son `0`).
    ///
    /// Args:
    ///     n: Numero de bits a leer (`0..=64`).
    ///
    /// Returns:
    ///     Valor leido.
    fn read(&mut self, n: u32) -> u64 {
        while self.used < n {
            let byte = self.bytes.get(self.cursor).copied().unwrap_or(0);
            self.acc |= u128::from(byte) << self.used;
            self.cursor += 1;
            self.used += 8;
        }
        let mask = if n == 64 {
            u128::from(u64::MAX)
        } else {
            (1u128 << n) - 1
        };
        let value = (self.acc & mask) as u64;
        // Desplaza (no pone a cero): pueden quedar bits pendientes del campo
        // siguiente cuando `used > n` (p. ej. `used = 71`, `n = 64`).
        self.acc >>= n;
        self.used -= n;
        value
    }
}

/// Compresor/descompresor Gorilla para `f64`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GorillaTs;

impl GorillaTs {
    /// Crea el codec.
    ///
    /// Returns:
    ///     Instancia sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// Comprime una serie de `f64`.
    ///
    /// Formato: `u32 n` (LE) + primer valor crudo (64 bits) + bits de control
    /// Gorilla. El primer valor no se comprime porque no tiene predecesor.
    ///
    /// Args:
    ///     values: Serie temporal (no necesita ser monotona).
    ///
    /// Returns:
    ///     Buffer comprimido; `decode` recupera los bits originales.
    pub fn encode(&self, values: &[f64]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + values.len());
        out.extend_from_slice(&(values.len() as u32).to_le_bytes());
        let Some((&first, rest)) = values.split_first() else {
            return out;
        };

        let mut bits = BitStream::new();
        bits.write(first.to_bits(), 64);
        let mut prev = first.to_bits();
        let mut prev_leading = u32::MAX;
        let mut prev_trailing = 0u32;

        for &value in rest {
            let current = value.to_bits();
            let xor = current ^ prev;
            prev = current;
            if xor == 0 {
                bits.write(0, 1);
                continue;
            }
            bits.write(1, 1);
            let leading = xor.leading_zeros().min(31);
            let trailing = xor.trailing_zeros().min(63);
            let meaningful = 64 - leading - trailing;

            if prev_leading != u32::MAX && leading >= prev_leading && trailing >= prev_trailing {
                // Reutiliza la ventana anterior: 1 bit de control + datos.
                bits.write(0, 1);
                bits.write(xor >> prev_trailing, 64 - prev_leading - prev_trailing);
            } else {
                // Nueva ventana: 5 bits de leading + 6 bits de longitud-1.
                bits.write(1, 1);
                bits.write(u64::from(leading), 5);
                bits.write(u64::from(meaningful - 1), 6);
                bits.write(xor >> trailing, meaningful);
                prev_leading = leading;
                prev_trailing = trailing;
            }
        }
        out.extend_from_slice(&bits.finish());
        out
    }

    /// Descomprime una serie producida por [`Self::encode`].
    ///
    /// Args:
    ///     bytes: Buffer comprimido.
    ///
    /// Returns:
    ///     Serie original. Un buffer truncado produce ceros (sin panic); la
    ///     integridad la aporta el checksum de pagina.
    pub fn decode(&self, bytes: &[u8]) -> Vec<f64> {
        if bytes.len() < 4 {
            return Vec::new();
        }
        let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        if n == 0 {
            return Vec::new();
        }
        let mut reader = BitReader::new(&bytes[4..]);
        let mut out = Vec::with_capacity(n);
        let mut prev = reader.read(64);
        out.push(f64::from_bits(prev));
        let mut prev_leading = u32::MAX;
        let mut prev_trailing = 0u32;

        while out.len() < n {
            if reader.read(1) == 0 {
                out.push(f64::from_bits(prev));
                continue;
            }
            let (leading, trailing) = if reader.read(1) == 0 && prev_leading != u32::MAX {
                (prev_leading, prev_trailing)
            } else {
                let leading = reader.read(5) as u32;
                let meaningful = reader.read(6) as u32 + 1;
                let trailing = 64 - leading - meaningful;
                prev_leading = leading;
                prev_trailing = trailing;
                (leading, trailing)
            };
            let meaningful = 64 - leading - trailing;
            let xor = reader.read(meaningful) << trailing;
            prev ^= xor;
            out.push(f64::from_bits(prev));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::GorillaTs;

    /// AC-0178-01: una serie constante se comprime por debajo de 1 byte/valor.
    #[test]
    // @spec AC-0178-01
    fn test_ac_0178_01_ratio() {
        let g = GorillaTs::new();
        let constant = vec![42.5f64; 1000];
        let raw = g.encode(&constant);
        assert!(
            raw.len() < 1000,
            "constante debe ocupar <1 byte/valor, ocupa {}",
            raw.len()
        );

        // Serie de sensor estable (1 cambio cada 20 lecturas): Gorilla debe
        // quedar muy por debajo de los 8 bytes/valor de `f64` crudo.
        let mut level = 20.0f64;
        let sensor: Vec<f64> = (0..1000)
            .map(|i| {
                if i % 20 == 0 {
                    level += 0.5;
                }
                level
            })
            .collect();
        let packed = g.encode(&sensor);
        assert!(
            packed.len() < 1000 * 2,
            "sensor estable debe ocupar <2 bytes/valor: {} bytes",
            packed.len()
        );
    }

    /// AC-0178-02: round-trip bit-exacto, incluidos valores patologicos.
    #[test]
    // @spec AC-0178-02
    fn test_ac_0178_02_lossless() {
        let g = GorillaTs::new();
        let values = vec![
            0.0f64,
            -0.0,
            1.0,
            -1.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            f64::from_bits(0x7FF8_0000_0000_0001),
            1.000_000_000_000_000_2,
            -std::f64::consts::PI,
            1e-300,
            1e300,
        ];
        let packed = g.encode(&values);
        let restored = g.decode(&packed);
        assert_eq!(restored.len(), values.len());
        let original: Vec<u64> = values.iter().map(|v| v.to_bits()).collect();
        let decoded: Vec<u64> = restored.iter().map(|v| v.to_bits()).collect();
        assert_eq!(original, decoded, "los bits deben sobrevivir al codec");
    }

    /// AC-0178-02 (borde): buffer truncado no hace panic.
    #[test]
    fn truncated_stream_does_not_panic() {
        let g = GorillaTs::new();
        let packed = g.encode(&[1.0, 2.0, 3.0, 4.0]);
        for cut in 0..packed.len() {
            let out = g.decode(&packed[..cut]);
            assert!(out.len() <= 4);
        }
    }
}
