//! Inferencia ONNX opcional (SPEC-0111): MLP puro en Rust, sin `ort`.
//!
//! Modelo de dos capas `y = softmax(W2 · ReLU(W1 · x + b1) + b2)` con pesos
//! deterministas (o inyectados). Cubre clasificacion/embedding ligera en edge
//! sin dependencias externas; la integracion con `ort` para transformers queda
//! como adapter futuro.

/// Perceptron multicapa (2 capas) con activacion ReLU y salida softmax.
#[derive(Debug, Clone)]
pub struct Mlp {
    input_dim: usize,
    hidden_dim: usize,
    output_dim: usize,
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: Vec<f32>,
}

/// `splitmix64` para pesos deterministas.
fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Peso determinista en `[-0.5, 0.5)`.
fn weight(seed: u64) -> f32 {
    let value = splitmix64(seed) >> 40; // 24 bits
    (value as f32 / (1u64 << 24) as f32) - 0.5
}

impl Mlp {
    /// Crea un MLP con pesos deterministas pseudoaleatorios.
    ///
    /// Args:
    ///     input_dim: Dimension de entrada.
    ///     hidden_dim: Neuronas ocultas.
    ///     output_dim: Clases/salidas.
    ///
    /// Returns:
    ///     Modelo listo.
    pub fn new(input_dim: usize, hidden_dim: usize, output_dim: usize) -> Self {
        let w1 = (0..input_dim * hidden_dim)
            .map(|index| weight(index as u64 + 1))
            .collect();
        let b1 = (0..hidden_dim)
            .map(|index| weight(index as u64 + 10_000))
            .collect();
        let w2 = (0..hidden_dim * output_dim)
            .map(|index| weight(index as u64 + 20_000))
            .collect();
        let b2 = (0..output_dim)
            .map(|index| weight(index as u64 + 30_000))
            .collect();
        Self {
            input_dim,
            hidden_dim,
            output_dim,
            w1,
            b1,
            w2,
            b2,
        }
    }

    /// Crea un MLP con pesos explicitos.
    ///
    /// Args:
    ///     input_dim: Dimension de entrada.
    ///     hidden_dim: Neuronas ocultas.
    ///     output_dim: Salidas.
    ///     w1: `input_dim * hidden_dim` pesos (row-major por neurona).
    ///     b1: `hidden_dim` sesgos.
    ///     w2: `hidden_dim * output_dim` pesos.
    ///     b2: `output_dim` sesgos.
    ///
    /// Returns:
    ///     `Ok(modelo)` si las longitudes cuadran.
    ///
    /// Errors:
    ///     `Err(String)` con el tamano esperado.
    pub fn with_weights(
        input_dim: usize,
        hidden_dim: usize,
        output_dim: usize,
        w1: Vec<f32>,
        b1: Vec<f32>,
        w2: Vec<f32>,
        b2: Vec<f32>,
    ) -> Result<Self, String> {
        if w1.len() != input_dim * hidden_dim {
            return Err(format!("w1: {} != {}*{}", w1.len(), input_dim, hidden_dim));
        }
        if b1.len() != hidden_dim || b2.len() != output_dim || w2.len() != hidden_dim * output_dim {
            return Err("dimensiones de b1/b2/w2 inconsistentes".to_string());
        }
        Ok(Self {
            input_dim,
            hidden_dim,
            output_dim,
            w1,
            b1,
            w2,
            b2,
        })
    }

    /// Dimension de entrada.
    ///
    /// Returns:
    ///     `input_dim`.
    pub fn input_dim(&self) -> usize {
        self.input_dim
    }

    /// Dimension de salida.
    ///
    /// Returns:
    ///     `output_dim`.
    pub fn output_dim(&self) -> usize {
        self.output_dim
    }

    /// Logits (sin softmax).
    ///
    /// Args:
    ///     input: Vector de `input_dim` floats.
    ///
    /// Returns:
    ///     `output_dim` logits.
    ///
    /// Errors:
    ///     `Err(String)` si la dimension no coincide.
    pub fn infer_logits(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        if input.len() != self.input_dim {
            return Err(format!(
                "dimension de entrada {} != {} (modelo)",
                input.len(),
                self.input_dim
            ));
        }
        let mut hidden = vec![0.0f32; self.hidden_dim];
        for (neuron, slot) in hidden.iter_mut().enumerate() {
            let base = neuron * self.input_dim;
            let mut sum = self.b1[neuron];
            for (index, value) in input.iter().enumerate() {
                sum += value * self.w1[base + index];
            }
            *slot = sum.max(0.0); // ReLU
        }
        let mut logits = vec![0.0f32; self.output_dim];
        for (output, slot) in logits.iter_mut().enumerate() {
            let base = output * self.hidden_dim;
            let mut sum = self.b2[output];
            for (neuron, value) in hidden.iter().enumerate() {
                sum += value * self.w2[base + neuron];
            }
            *slot = sum;
        }
        Ok(logits)
    }

    /// Softmax numericamente estable.
    ///
    /// Args:
    ///     input: Vector de entrada.
    ///
    /// Returns:
    ///     Probabilidades que suman 1.
    ///
    /// Errors:
    ///     Las mismas que [`Self::infer_logits`].
    pub fn infer(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        let logits = self.infer_logits(input)?;
        Ok(softmax(&logits))
    }

    /// Clase predicha (argmax de las probabilidades).
    ///
    /// Args:
    ///     input: Vector de entrada.
    ///
    /// Returns:
    ///     Indice de la clase con mayor probabilidad.
    ///
    /// Errors:
    ///     Las mismas que [`Self::infer`].
    pub fn predict(&self, input: &[f32]) -> Result<usize, String> {
        let probabilities = self.infer(input)?;
        Ok(probabilities
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .unwrap_or(0))
    }
}

/// Softmax estable (`exp(x - max)`).
fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut probabilities: Vec<f32> = logits.iter().map(|value| (value - max).exp()).collect();
    let sum: f32 = probabilities.iter().sum();
    if sum > 0.0 {
        for probability in &mut probabilities {
            *probability /= sum;
        }
    }
    probabilities
}

/// Modelo ONNX (MLP puro en Rust).
#[derive(Debug, Clone)]
pub struct OnnxModel {
    path: String,
    output_dim: usize,
    linear: Option<(Vec<f32>, f32)>,
    mlp: Option<Mlp>,
}

impl OnnxModel {
    /// Carga un modelo (MLP determinista de 2 -> 4 -> 4).
    ///
    /// Args:
    ///     path: Ruta del modelo (informativa).
    ///
    /// Returns:
    ///     Modelo listo.
    pub fn load(path: impl Into<String>) -> Result<Self, String> {
        Ok(Self {
            path: path.into(),
            output_dim: 4,
            linear: None,
            mlp: Some(Mlp::new(2, 4, 4)),
        })
    }

    /// Carga un modelo con dimension de salida explicita.
    ///
    /// Args:
    ///     path: Ruta.
    ///     output_dim: Salidas del MLP.
    ///
    /// Returns:
    ///     Modelo `(2 -> 4 -> output_dim)`.
    pub fn load_with_dim(path: impl Into<String>, output_dim: usize) -> Result<Self, String> {
        if output_dim == 0 {
            return Err("output_dim debe ser >= 1".to_string());
        }
        Ok(Self {
            path: path.into(),
            output_dim,
            linear: None,
            mlp: Some(Mlp::new(2, 4, output_dim)),
        })
    }

    /// Carga un modelo lineal con pesos y sesgo explicitos (compatibilidad).
    ///
    /// Args:
    ///     path: Ruta.
    ///     output_dim: Salidas.
    ///     weights: Pesos del dot product.
    ///     bias: Sesgo.
    ///
    /// Returns:
    ///     Modelo con la ruta lineal.
    pub fn load_with_weights(
        path: impl Into<String>,
        output_dim: usize,
        weights: Vec<f32>,
        bias: f32,
    ) -> Result<Self, String> {
        Ok(Self {
            path: path.into(),
            output_dim,
            linear: Some((weights, bias)),
            mlp: None,
        })
    }

    /// Ruta del modelo.
    ///
    /// Returns:
    ///     Ruta declarada.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Dimension de salida.
    ///
    /// Returns:
    ///     Salidas.
    pub fn output_dim(&self) -> usize {
        self.output_dim
    }

    /// Inferencia (MLP + softmax, o ceros si no hay MLP).
    ///
    /// Args:
    ///     input: Vector de entrada.
    ///
    /// Returns:
    ///     Probabilidades de `output_dim`.
    ///
    /// Errors:
    ///     `Err(String)` si la dimension no coincide.
    pub fn infer(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        match &self.mlp {
            Some(mlp) => mlp.infer(input),
            None => Ok(vec![0.0; self.output_dim]),
        }
    }

    /// Inferencia lineal (dot product + bias), ruta de compatibilidad.
    ///
    /// Args:
    ///     input: Vector de entrada.
    ///
    /// Returns:
    ///     `output_dim` copias de `dot + bias`.
    ///
    /// Errors:
    ///     `Err(String)` si falta la ruta lineal o la dimension no coincide.
    pub fn infer_linear(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        let Some((weights, bias)) = &self.linear else {
            return Err("el modelo no tiene ruta lineal (usa infer)".to_string());
        };
        if input.len() != weights.len() {
            return Err(format!(
                "dimension de entrada {} != {} (pesos)",
                input.len(),
                weights.len()
            ));
        }
        let dot: f32 = input.iter().zip(weights.iter()).map(|(a, b)| a * b).sum();
        Ok(vec![dot + bias; self.output_dim])
    }

    /// Inferencia MLP con normalizacion L2.
    ///
    /// Args:
    ///     input: Vector de entrada.
    ///
    /// Returns:
    ///     Salida normalizada.
    ///
    /// Errors:
    ///     Las mismas que [`Self::infer`].
    pub fn infer_normalized(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        let mut out = self.infer(input)?;
        let norm: f32 = out.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut out {
                *value /= norm;
            }
        }
        Ok(out)
    }

    /// Inferencia lineal con normalizacion L2.
    ///
    /// Args:
    ///     input: Vector de entrada.
    ///
    /// Returns:
    ///     Salida normalizada.
    ///
    /// Errors:
    ///     Las mismas que [`Self::infer_linear`].
    pub fn infer_linear_normalized(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        let mut out = self.infer_linear(input)?;
        let norm: f32 = out.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut out {
                *value /= norm;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{Mlp, OnnxModel};

    #[test] // @spec AC-0111-01
    fn test_ac_0111_01_onnx_load_safe() {
        let model = OnnxModel::load("modelo.onnx").expect("load");
        assert_eq!(model.path(), "modelo.onnx");
        assert_eq!(model.output_dim(), 4);
        let out = model.infer(&[1.0, 2.0]).expect("infer");
        assert_eq!(out.len(), 4);
        let norm = model
            .infer_normalized(&[1.0, 2.0])
            .expect("infer normalized");
        assert_eq!(norm.len(), 4);
    }

    #[test]
    fn test_onnx_linear() {
        let model = OnnxModel::load_with_weights("m.onnx", 2, vec![1.0, 2.0], 0.5).expect("load");
        let out = model.infer_linear(&[3.0, 4.0]).expect("infer linear");
        // dot = 3*1 + 4*2 = 11, + bias 0.5 = 11.5
        assert!((out[0] - 11.5).abs() < 1e-6);
        assert!((out[1] - 11.5).abs() < 1e-6);
    }

    #[test]
    fn test_onnx_linear_dim_mismatch() {
        let model = OnnxModel::load_with_weights("m.onnx", 2, vec![1.0, 2.0], 0.0).expect("load");
        assert!(model.infer_linear(&[1.0]).is_err());
    }

    /// El MLP devuelve probabilidades validas y prediccion determinista.
    #[test]
    fn mlp_softmax_and_prediction() {
        let mlp = Mlp::new(3, 5, 4);
        let probabilities = mlp.infer(&[1.0, -2.0, 0.5]).expect("infer");
        assert_eq!(probabilities.len(), 4);
        let sum: f32 = probabilities.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5, "suma softmax: {sum}");
        assert!(probabilities.iter().all(|p| *p >= 0.0 && *p <= 1.0));
        let first = mlp.predict(&[1.0, -2.0, 0.5]).expect("predict");
        let second = mlp.predict(&[1.0, -2.0, 0.5]).expect("predict");
        assert_eq!(first, second, "determinista");
        assert!(first < 4, "indice valido");
        assert!(mlp.infer(&[1.0]).is_err(), "entrada de dimension distinta");

        // Pesos explicitos: identidad en la capa oculta.
        let explicit = Mlp::with_weights(
            2,
            2,
            2,
            vec![1.0, 0.0, 0.0, 1.0],
            vec![0.0, 0.0],
            vec![1.0, 0.0, 0.0, 1.0],
            vec![0.0, 0.0],
        )
        .expect("weights");
        let logits = explicit.infer_logits(&[1.0, -1.0]).expect("logits");
        assert_eq!(logits, vec![1.0, 0.0], "ReLU elimina el negativo");
        assert!(Mlp::with_weights(2, 2, 2, vec![1.0], vec![], vec![], vec![]).is_err());
    }
}
