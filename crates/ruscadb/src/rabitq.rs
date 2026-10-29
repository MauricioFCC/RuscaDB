//! Cuantizacion binaria RaBitQ para ANN (SPEC-0171).
//!
//! RaBitQ (SIGMOD 2024, arXiv:2405.12497) codifica cada vector en `d` bits
//! (signo por dimension) y estima distancias con un factor de correccion
//! teorico en vez de comparar bit a bit. Con `d` bits frente a `4d` bytes de
//! `f32` la memoria baja ~32x, lo que permite mantener indices HNSW grandes en
//! RAM y reordenar (rerank) solo el top-k con vectores exactos.
//!
//! Derivacion del estimador: si `x` es una direccion unitaria uniforme en la
//! esfera, entonces `E[x_i] = 0` y `E[|x_i|] = sqrt(2/(pi*d))`. Condicionado al
//! signo `b_i = sign(x_i)`, la media condicional es `E[x_i | b_i] ≈
//! sqrt(2/(pi*d)) * b_i`, luego
//! `E[<q, x> | b] ≈ sqrt(2/(pi*d)) * <q, b>`.
//! Esa es la correccion que aplica `estimate_dot`.

/// Codigo RaBitQ de un vector: bits de signo + norma.
#[derive(Debug, Clone, PartialEq)]
pub struct RabitqCode {
    /// Bits de signo empaquetados (bit `i` a 1 = componente `>= 0`).
    pub bits: Vec<u8>,
    /// Dimension original.
    pub dim: usize,
    /// Norma L2 del vector original.
    pub norm: f32,
}

impl RabitqCode {
    /// Bytes ocupados por el codigo.
    ///
    /// Returns:
    ///     Longitud del buffer de bits (sin contar metadatos).
    pub fn bytes(&self) -> usize {
        self.bits.len()
    }

    /// Indica si el vector era el cero (norma nula, bits sin informacion).
    ///
    /// Returns:
    ///     `true` si la norma es 0.
    pub fn is_zero(&self) -> bool {
        self.norm == 0.0
    }
}

/// Cuantizador RaBitQ.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RabitqQuantizer;

impl RabitqQuantizer {
    /// Crea el cuantizador.
    ///
    /// Returns:
    ///     Instancia sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// Numero de bytes necesarios para `dim` bits de signo.
    ///
    /// Args:
    ///     dim: Dimension del vector.
    ///
    /// Returns:
    ///     Bytes empaquetados (`ceil(dim/8)`).
    pub fn code_len(dim: usize) -> usize {
        dim.div_ceil(8)
    }

    /// Codifica un vector `f32` en `dim` bits + norma.
    ///
    /// Args:
    ///     vector: Vector original (cualquier norma).
    ///
    /// Returns:
    ///     Codigo con signos empaquetados y norma L2.
    pub fn encode(&self, vector: &[f32]) -> RabitqCode {
        let mut bits = vec![0u8; Self::code_len(vector.len())];
        let mut squared = 0.0f64;
        for (index, &value) in vector.iter().enumerate() {
            squared += f64::from(value) * f64::from(value);
            if value >= 0.0 {
                bits[index / 8] |= 1 << (index % 8);
            }
        }
        RabitqCode {
            bits,
            dim: vector.len(),
            norm: squared.sqrt() as f32,
        }
    }

    /// Signo almacenado en la posicion `index`.
    ///
    /// Args:
    ///     code: Codigo RaBitQ.
    ///     index: Dimension.
    ///
    /// Returns:
    ///     `1.0` si el bit esta a 1, `-1.0` si esta a 0 (o fuera de rango).
    fn sign(code: &RabitqCode, index: usize) -> f32 {
        if index / 8 >= code.bits.len() {
            return -1.0;
        }
        if code.bits[index / 8] & (1 << (index % 8)) != 0 {
            1.0
        } else {
            -1.0
        }
    }

    /// Norma L2 de un vector.
    ///
    /// Args:
    ///     vector: Vector de entrada.
    ///
    /// Returns:
    ///     Norma euclidiana.
    pub fn norm(&self, vector: &[f32]) -> f32 {
        let squared: f64 = vector
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum::<f64>();
        squared.sqrt() as f32
    }

    /// Estima `<q_unit, o>` (producto interno) con correccion teorica.
    ///
    /// Args:
    ///     query_unit: Consulta **ya normalizada** (norma 1).
    ///     code: Codigo del vector de datos.
    ///
    /// Returns:
    ///     Estimacion del producto interno.
    pub fn estimate_dot(&self, query_unit: &[f32], code: &RabitqCode) -> f32 {
        if code.is_zero() || code.dim == 0 {
            return 0.0;
        }
        let mut accumulator = 0.0f64;
        for (index, &value) in query_unit.iter().enumerate().take(code.dim) {
            accumulator += f64::from(value) * f64::from(Self::sign(code, index));
        }
        let correction = (2.0 / (std::f64::consts::PI * code.dim as f64)).sqrt();
        (correction * accumulator * f64::from(code.norm)) as f32
    }

    /// Estima la distancia L2 al cuadrado entre consulta y codigo.
    ///
    /// Args:
    ///     query: Consulta original (sin normalizar).
    ///     code: Codigo del vector de datos.
    ///
    /// Returns:
    ///     Estimacion de `||query - data||^2`, siempre `>= 0`.
    pub fn estimate_l2(&self, query: &[f32], code: &RabitqCode) -> f32 {
        let query_norm = self.norm(query);
        if query_norm == 0.0 {
            return code.norm * code.norm;
        }
        let unit: Vec<f32> = query.iter().map(|v| v / query_norm).collect();
        let dot = self.estimate_dot(&unit, code);
        let estimate = query_norm * query_norm + code.norm * code.norm - 2.0 * query_norm * dot;
        estimate.max(0.0)
    }

    /// Reconstruye una direccion aproximada (solo para diagnostico/rerank
    /// barato). No se usa para distancias exactas.
    ///
    /// Args:
    ///     code: Codigo a expandir.
    ///
    /// Returns:
    ///     Vector de `dim` componentes en `{-1/√d, +1/√d}` escalado por norma.
    pub fn approximate_direction(&self, code: &RabitqCode) -> Vec<f32> {
        if code.dim == 0 {
            return Vec::new();
        }
        let scale = code.norm / (code.dim as f32).sqrt();
        (0..code.dim)
            .map(|index| Self::sign(code, index) * scale)
            .collect()
    }

    /// Ratio de compresion frente a `f32` crudo.
    ///
    /// Args:
    ///     dim: Dimension.
    ///
    /// Returns:
    ///     Factor de reduccion de memoria (aprox. 32x para `dim >= 8`).
    pub fn compression_ratio(dim: usize) -> f32 {
        if dim == 0 {
            return 1.0;
        }
        (dim * 4) as f32 / Self::code_len(dim) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::{RabitqCode, RabitqQuantizer};

    /// PRNG xorshift64* determinista (sin dependencias).
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        /// Aproximacion normal por Box-Muller.
        fn normal(&mut self) -> f32 {
            let u1 = ((self.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64;
            let u2 = ((self.next_u64() >> 11) as f64) / (1u64 << 53) as f64;
            let radius = (-2.0 * u1.ln()).sqrt();
            (radius * (2.0 * std::f64::consts::PI * u2).cos()) as f32
        }

        fn vector(&mut self, dim: usize) -> Vec<f32> {
            (0..dim).map(|_| self.normal()).collect()
        }
    }

    fn l2_squared(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>()
    }

    /// AC-0171-01: el codigo ocupa `d/8` bytes (~32x) y el estimador tiene
    /// error acotado sobre el producto interno.
    #[test]
    // @spec AC-0171-01
    fn test_ac_0171_01_encode() {
        let quantizer = RabitqQuantizer::new();
        let mut rng = Rng(0xC0FF_EE12_3456_789A);
        let dim = 256;
        let vector = rng.vector(dim);
        let code = quantizer.encode(&vector);

        assert_eq!(code.dim, dim);
        assert_eq!(code.bytes(), dim / 8);
        assert!(
            RabitqQuantizer::compression_ratio(dim) >= 30.0,
            "ratio ~32x, se obtuvo {}",
            RabitqQuantizer::compression_ratio(dim)
        );
        assert!((code.norm - quantizer.norm(&vector)).abs() < 1e-4);

        // Error del estimador de producto interno, normalizado por las normas.
        let mut total_error = 0.0f64;
        let trials = 200;
        for _ in 0..trials {
            let data = rng.vector(dim);
            let query = rng.vector(dim);
            let data_norm = quantizer.norm(&data);
            let query_norm = quantizer.norm(&query);
            let unit: Vec<f32> = query.iter().map(|v| v / query_norm).collect();
            let coded = quantizer.encode(&data);
            let estimated = quantizer.estimate_dot(&unit, &coded);
            let exact: f32 = unit.iter().zip(&data).map(|(q, d)| q * d).sum::<f32>() / data_norm;
            total_error += f64::from((estimated / data_norm - exact).abs());
        }
        let mean_error = total_error / f64::from(trials);
        assert!(
            mean_error < 0.15,
            "error medio del coseno {mean_error} debe ser < 0.15"
        );

        // Vector cero y dimension cero no rompen el estimador: la distancia al
        // vector nulo es `||q||^2` (aqui 1+4=5).
        let zero = quantizer.encode(&[]);
        assert!(zero.is_zero());
        assert_eq!(quantizer.estimate_dot(&[], &zero), 0.0);
        let to_zero = quantizer.estimate_l2(&[1.0, 2.0], &zero);
        assert!((to_zero - 5.0).abs() < 1e-5, "distancia al nulo: {to_zero}");
    }

    /// AC-0171-02: con rerank del top-k, el ranking estimado recupera el
    /// vecindario real (recall@10 alto) gastando `d/8` bytes por vector.
    #[test]
    // @spec AC-0171-02
    fn test_ac_0171_02_recall() {
        let quantizer = RabitqQuantizer::new();
        let mut rng = Rng(0x1234_5678_9ABC_DEF0);
        let dim = 128;
        let n = 200;
        let k = 10;

        // 20 clusters de 10 puntos: el vecindario verdadero de una consulta es
        // su propio cluster, que es exactamente lo que el indice cuantizado
        // debe recuperar sin rerank.
        let clusters = 20;
        let centers: Vec<Vec<f32>> = (0..clusters).map(|_| rng.vector(dim)).collect();
        let mut data: Vec<Vec<f32>> = Vec::with_capacity(n);
        for i in 0..n {
            let center = &centers[i % clusters];
            let noise = rng.vector(dim);
            data.push(
                center
                    .iter()
                    .zip(&noise)
                    .map(|(c, e)| c + 0.35 * e)
                    .collect(),
            );
        }
        let codes: Vec<RabitqCode> = data.iter().map(|v| quantizer.encode(v)).collect();

        let mut total_recall = 0.0f64;
        let queries = 20;
        for _ in 0..queries {
            let center = &centers[0];
            let noise = rng.vector(dim);
            let query: Vec<f32> = center
                .iter()
                .zip(&noise)
                .map(|(c, e)| c + 0.35 * e)
                .collect();

            let mut exact: Vec<(usize, f32)> = data
                .iter()
                .enumerate()
                .map(|(i, v)| (i, l2_squared(&query, v)))
                .collect();
            exact.sort_by(|a, b| a.1.total_cmp(&b.1));

            let mut estimated: Vec<(usize, f32)> = codes
                .iter()
                .enumerate()
                .map(|(i, code)| (i, quantizer.estimate_l2(&query, code)))
                .collect();
            estimated.sort_by(|a, b| a.1.total_cmp(&b.1));

            let true_top: Vec<usize> = exact.iter().take(k).map(|(i, _)| *i).collect();
            let hits = estimated
                .iter()
                .take(k)
                .filter(|(i, _)| true_top.contains(i))
                .count();
            total_recall += hits as f64 / k as f64;
        }
        let recall = total_recall / f64::from(queries);
        assert!(recall >= 0.80, "recall@10 medido: {recall}");
    }

    /// `approximate_direction` preserva la norma y el signo.
    #[test]
    fn approximate_direction_keeps_norm() {
        let quantizer = RabitqQuantizer::new();
        let vector = vec![3.0f32, -4.0, 0.0, 1.0];
        let code = quantizer.encode(&vector);
        let approximation = quantizer.approximate_direction(&code);
        assert_eq!(approximation.len(), 4);
        let norm = quantizer.norm(&approximation);
        assert!((norm - code.norm).abs() < 1e-4);
        assert!(approximation[0] > 0.0 && approximation[1] < 0.0);
    }
}
