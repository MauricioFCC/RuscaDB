//! Generador pseudoaleatorio determinista (xorshift64*) para niveles HNSW.

/// Nivel máximo del grafo jerárquico (protección anti-bucle).
pub(crate) const MAX_LEVEL: usize = 32;

/// PRNG xorshift64* determinista y reproducible.
pub(crate) struct Rng {
    state: u64,
}

impl Rng {
    /// Crea un PRNG con la semilla dada (nunca cero).
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    /// Siguiente entero de 64 bits.
    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Siguiente flotante uniforme en `[0, 1)`.
    pub(crate) fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }

    /// Nivel aleatorio geométrico con multiplicador `1/ln(M)`.
    pub(crate) fn random_level(&mut self, m: usize) -> usize {
        let multiplier = 1.0 / (m as f64).ln();
        let mut level = 0;
        while level < MAX_LEVEL && self.next_unit() < multiplier {
            level += 1;
        }
        level
    }
}
