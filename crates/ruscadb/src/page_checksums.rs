//! Checksums de pagina con CRC-32C (Castagnoli), SPEC-0167.
//!
//! CRC-32C es el algoritmo que usan RocksDB, LevelDB, Kafka y ext4 para
//! detectar corrupcion de bloques. Se implementa aqui en Rust puro con tabla
//! generada en tiempo de compilacion (`const fn`), sin dependencias externas.
//!
//! Polinomio Castagnoli reflejado: `0x82F6_3B78` (forma normal `0x1EDC_6F41`).
//! Valor de verificacion conocido (RFC 3720 / iSCSI): `crc32c("123456789")`
//! debe ser `0xE306_9283`; el test lo comprueba de forma auto-verificable.

/// Polinomio Castagnoli en representacion reflejada (LSB-first).
const CASTAGNOLI_REFLECTED: u32 = 0x82F6_3B78;

/// Construye la tabla CRC-32C de 256 entradas en tiempo de compilacion.
///
/// Returns:
///     Tabla de 256 valores: `TABLE[b]` es el CRC de un unico byte `b`.
const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ CASTAGNOLI_REFLECTED
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Tabla CRC-32C precomputada.
const TABLE: [u32; 256] = build_table();

/// Calculadora de checksums de pagina CRC-32C.
///
/// Es un tipo sin estado (la tabla es `const`), por lo que `Copy` es seguro.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PageChecksum;

impl PageChecksum {
    /// Crea una calculadora.
    ///
    /// Returns:
    ///     Instancia sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// Acumula un bloque sobre un CRC parcial (streaming).
    ///
    /// Args:
    ///     seed: CRC parcial previo (`0` para empezar).
    ///     data: Bytes a acumular.
    ///
    /// Returns:
    ///     CRC parcial actualizado.
    pub fn update(&self, seed: u32, data: &[u8]) -> u32 {
        let mut crc = !seed;
        for &byte in data {
            let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
            crc = (crc >> 8) ^ TABLE[idx];
        }
        !crc
    }

    /// Calcula el CRC-32C de un bloque completo.
    ///
    /// Args:
    ///     data: Bytes a verificar.
    ///
    /// Returns:
    ///     CRC-32C de 32 bits.
    pub fn crc32c(&self, data: &[u8]) -> u32 {
        self.update(0, data)
    }

    /// Verifica un CRC esperado contra el calculado.
    ///
    /// Args:
    ///     data: Bytes originales.
    ///     expected: CRC almacenado (p. ej. leido del footer de la pagina).
    ///
    /// Returns:
    ///     `true` si coincide bit a bit.
    pub fn verify(&self, data: &[u8], expected: u32) -> bool {
        self.crc32c(data) == expected
    }

    /// Sella una pagina escribiendo su CRC en los ultimos 4 bytes (LE).
    ///
    /// Los ultimos 4 bytes se excluyen del calculo, de modo que
    /// [`Self::verify_stamped`] puede re-sellar sin ambiguedad.
    ///
    /// Args:
    ///     page: Pagina de al menos 4 bytes; se modifica in situ.
    ///
    /// Returns:
    ///     `true` si la pagina tenia tamano suficiente y quedo sellada.
    pub fn stamp(&self, page: &mut [u8]) -> bool {
        let Some(split) = page.len().checked_sub(4) else {
            return false;
        };
        let (payload, footer) = page.split_at_mut(split);
        let crc = self.crc32c(payload);
        footer.copy_from_slice(&crc.to_le_bytes());
        true
    }

    /// Verifica una pagina sellada con [`Self::stamp`].
    ///
    /// Args:
    ///     page: Pagina con CRC en los ultimos 4 bytes.
    ///
    /// Returns:
    ///     `true` si el CRC coincide; `false` si hay corrupcion, torn write o
    ///     la pagina es demasiado corta (no se puede distinguir un sector
    ///     truncado de uno corrupto, ambos son invalidos).
    pub fn verify_stamped(&self, page: &[u8]) -> bool {
        let Some(split) = page.len().checked_sub(4) else {
            return false;
        };
        let (payload, footer) = page.split_at(split);
        let expected = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]);
        self.verify(payload, expected)
    }
}

#[cfg(test)]
mod tests {
    use super::PageChecksum;

    /// AC-0167-01: vector de verificacion estandar + sello/verificacion.
    #[test]
    // @spec AC-0167-01
    fn test_ac_0167_01_valid() {
        let c = PageChecksum::new();
        // Vector publicado en RFC 3720 (iSCSI) para CRC-32C.
        assert_eq!(c.crc32c(b"123456789"), 0xE306_9283);

        let mut page = [0u8; 4096];
        page[0] = 0xAB;
        page[4095 - 4] = 0x11;
        assert!(c.stamp(&mut page), "pagina de 4096 debe sellarse");
        assert!(c.verify_stamped(&page), "sello recien escrito debe validar");
    }

    /// AC-0167-02: un bit volteado invalida el checksum (no hay colisiones
    /// triviales como con el fold anterior).
    #[test]
    // @spec AC-0167-02
    fn test_ac_0167_02_corrupt() {
        let c = PageChecksum::new();
        let mut page = vec![7u8; 512];
        assert!(c.stamp(&mut page));
        assert!(c.verify_stamped(&page));

        for byte_idx in [0usize, 255, 511 - 4] {
            let mut corrupt = page.clone();
            corrupt[byte_idx] ^= 0b0000_0001;
            assert!(
                !c.verify_stamped(&corrupt),
                "un bit volteado en {byte_idx} debe detectarse"
            );
        }
        // El mismo payload con otro CRC esperado no valida.
        assert!(!c.verify(b"hello", 999));
    }

    /// AC-0167-03: torn write / pagina truncada se rechaza.
    #[test]
    // @spec AC-0167-03
    fn test_ac_0167_03_torn() {
        let c = PageChecksum::new();
        // Pagina de menos de 4 bytes: no hay footer posible.
        assert!(!c.verify_stamped(b""));
        assert!(!c.verify_stamped(b"abc"));
        let mut short = [0u8; 3];
        assert!(!c.stamp(&mut short), "no se puede sellar sin footer");

        // Sector truncado: el CRC del payload original no aparece.
        let mut page = vec![3u8; 64];
        assert!(c.stamp(&mut page));
        let truncated = &page[..32];
        assert!(!c.verify_stamped(truncated));
    }

    /// Streaming equivalente a one-shot (invariante de `update`).
    #[test]
    fn streaming_equals_one_shot() {
        let c = PageChecksum::new();
        let data: Vec<u8> = (0..=255u8).collect();
        let one_shot = c.crc32c(&data);
        let mut streamed = 0;
        for chunk in data.chunks(7) {
            streamed = c.update(streamed, chunk);
        }
        assert_eq!(one_shot, streamed);
        // El CRC de vacio es 0 (convencion de `!seed` en ambos extremos).
        assert_eq!(c.crc32c(b""), 0);
    }
}
