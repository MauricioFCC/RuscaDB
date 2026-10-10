//! Cuantización escalar + índice plano cuantizado (SPEC-0058, R3/D4).
//!
//! Fallback con budget duro: cuando el HNSW en RAM (`N·M·dim·4B`) excede el
//! presupuesto, el índice plano cuantizado (1 B/dim + codebook) mantiene
//! recall@10 ≥ 0.95 con búsqueda asimétrica (query f32 vs códigos u8).

use ruscadb_core::RuscaError;

/// Cuantizador escalar uniforme por dimensión (min-max, 256 niveles).
#[derive(Clone, Debug)]
pub struct ScalarQuantizer {
    /// Dimensión de los vectores.
    dim: usize,
    /// Mínimo por dimensión (codebook).
    mins: Vec<f32>,
    /// Máximo por dimensión (codebook).
    maxs: Vec<f32>,
}

impl ScalarQuantizer {
    /// Ajusta el codebook (min/max por dimensión) al dataset.
    ///
    /// Args:
    ///     vectors: Vectores de entrenamiento, todos de igual dimensión.
    ///
    /// Returns:
    ///     El cuantizador ajustado.
    ///
    /// Raises:
    ///     [`RuscaError::InvalidConfig`] si el dataset está vacío;
    ///     [`RuscaError::DimensionMismatch`] si hay dims inconsistentes.
    pub fn fit(vectors: &[Vec<f32>]) -> Result<Self, RuscaError> {
        let first = vectors
            .first()
            .ok_or_else(|| RuscaError::InvalidConfig("fit exige al menos un vector".to_string()))?;
        let dim = first.len();
        if dim == 0 {
            return Err(RuscaError::InvalidConfig(
                "fit exige dimensión >= 1".to_string(),
            ));
        }
        let mut mins = vec![f32::INFINITY; dim];
        let mut maxs = vec![f32::NEG_INFINITY; dim];
        for vector in vectors {
            if vector.len() != dim {
                return Err(RuscaError::DimensionMismatch {
                    expected: dim,
                    actual: vector.len(),
                });
            }
            for (index, value) in vector.iter().enumerate() {
                mins[index] = mins[index].min(*value);
                maxs[index] = maxs[index].max(*value);
            }
        }
        Ok(Self { dim, mins, maxs })
    }

    /// Dimensión del cuantizador.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Codifica un vector a 1 byte por dimensión.
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si la dimensión no coincide.
    pub fn encode(&self, vector: &[f32]) -> Result<Vec<u8>, RuscaError> {
        if vector.len() != self.dim {
            return Err(RuscaError::DimensionMismatch {
                expected: self.dim,
                actual: vector.len(),
            });
        }
        Ok(vector
            .iter()
            .enumerate()
            .map(|(index, value)| self.encode_component(index, *value))
            .collect())
    }

    /// Decodifica un código al centroide de su celda.
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si la longitud no coincide.
    pub fn decode(&self, code: &[u8]) -> Result<Vec<f32>, RuscaError> {
        if code.len() != self.dim {
            return Err(RuscaError::DimensionMismatch {
                expected: self.dim,
                actual: code.len(),
            });
        }
        Ok(code
            .iter()
            .enumerate()
            .map(|(index, byte)| self.decode_component(index, *byte))
            .collect())
    }

    /// Distancia L2 asimétrica: query f32 contra código u8 (sin decodificar).
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si las longitudes no coinciden.
    pub fn asymmetric_l2(&self, query: &[f32], code: &[u8]) -> Result<f32, RuscaError> {
        if query.len() != self.dim || code.len() != self.dim {
            return Err(RuscaError::DimensionMismatch {
                expected: self.dim,
                actual: query.len().min(code.len()),
            });
        }
        let mut total = 0.0;
        for index in 0..self.dim {
            let approx = self.decode_component(index, code[index]);
            let diff = query[index] - approx;
            total += diff * diff;
        }
        Ok(total)
    }

    /// Codifica una componente al nivel 0..=255 más cercano.
    fn encode_component(&self, index: usize, value: f32) -> u8 {
        let span = self.maxs[index] - self.mins[index];
        if span > 0.0 {
            let scaled = (value - self.mins[index]) / span * 255.0;
            scaled.clamp(0.0, 255.0).round() as u8
        } else {
            0
        }
    }

    /// Decodifica un nivel a su centroide.
    fn decode_component(&self, index: usize, byte: u8) -> f32 {
        let span = self.maxs[index] - self.mins[index];
        if span > 0.0 {
            self.mins[index] + (byte as f32) / 255.0 * span
        } else {
            self.mins[index]
        }
    }
}

/// Índice plano sobre códigos cuantizados (fallback con budget duro).
#[derive(Clone, Debug)]
pub struct QuantizedFlatIndex {
    /// Dimensión de los vectores.
    dim: usize,
    /// Codebook ajustado al dataset.
    quantizer: ScalarQuantizer,
    /// Códigos en orden de inserción (id = posición).
    codes: Vec<Vec<u8>>,
}

impl QuantizedFlatIndex {
    /// Construye el índice cuantizando el dataset completo.
    ///
    /// Args:
    ///     vectors: Dataset (no vacío, dims consistentes).
    ///
    /// Returns:
    ///     El índice con un código por vector.
    ///
    /// Raises:
    ///     [`RuscaError::InvalidConfig`] si está vacío;
    ///     [`RuscaError::DimensionMismatch`] si hay inconsistencia.
    pub fn build(vectors: Vec<Vec<f32>>) -> Result<Self, RuscaError> {
        let quantizer = ScalarQuantizer::fit(&vectors)?;
        let dim = quantizer.dim();
        let mut codes = Vec::with_capacity(vectors.len());
        for vector in &vectors {
            codes.push(quantizer.encode(vector)?);
        }
        Ok(Self {
            dim,
            quantizer,
            codes,
        })
    }

    /// Dimensión del índice.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Número de vectores indexados.
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// `true` si no hay vectores (nunca ocurre tras `build`, por construcción).
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// Footprint real en bytes (códigos + codebook min/max).
    pub fn footprint_bytes(&self) -> usize {
        self.codes.len() * self.dim + 2 * self.dim * 4
    }

    /// Busca los `k` más cercanos por distancia asimétrica.
    ///
    /// Args:
    ///     query: Vector de consulta de dimensión `dim`.
    ///     k: Vecinos a devolver (`k == 0` => vacío).
    ///
    /// Returns:
    ///     `(id, distancia²)` ordenados ascendentemente.
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si la dimensión no coincide.
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<(usize, f32)>, RuscaError> {
        if query.len() != self.dim {
            return Err(RuscaError::DimensionMismatch {
                expected: self.dim,
                actual: query.len(),
            });
        }
        let mut scored: Vec<(usize, f32)> = Vec::with_capacity(self.codes.len());
        for (id, code) in self.codes.iter().enumerate() {
            scored.push((id, self.quantizer.asymmetric_l2(query, code)?));
        }
        scored.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        scored.truncate(k);
        Ok(scored)
    }
}
