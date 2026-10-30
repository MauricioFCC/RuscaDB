//! Inferencia ONNX opcional (SPEC-0111): stub sin dependencias externas.
//!
//! La integraciÃ³n real con `ort` queda como trabajo futuro cuando se requiera
//! inferencia transformer; este mÃ³dulo valida la API y los tests sin pulls.

/// Modelo ONNX (stub): carga y produce un vector.
#[derive(Debug, Clone)]
pub struct OnnxModel {
    /// Ruta del fichero del modelo (no se valida aquÃ­).
    path: String,
    /// DimensiÃ³n de salida declarada.
    output_dim: usize,
    /// Pesos del modelo lineal (stub).
    weights: Vec<f32>,
    /// Sesgo del modelo lineal (stub).
    bias: f32,
}

impl OnnxModel {
    /// Carga un modelo desde `path` (stub: no valida el fichero).
    pub fn load(path: impl Into<String>) -> Result<Self, String> {
        Ok(Self {
            path: path.into(),
            output_dim: 4,
            weights: vec![0.1, 0.2, 0.3, 0.4],
            bias: 0.0,
        })
    }

    /// Carga un modelo con dimensiÃ³n de salida explÃ­cita.
    pub fn load_with_dim(path: impl Into<String>, output_dim: usize) -> Result<Self, String> {
        Ok(Self {
            path: path.into(),
            output_dim,
            weights: vec![0.1; output_dim],
            bias: 0.0,
        })
    }

    /// Carga un modelo con pesos y sesgo explÃ­citos (stub para tests).
    pub fn load_with_weights(
        path: impl Into<String>,
        output_dim: usize,
        weights: Vec<f32>,
        bias: f32,
    ) -> Result<Self, String> {
        Ok(Self {
            path: path.into(),
            output_dim,
            weights,
            bias,
        })
    }

    /// Ruta del modelo.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// DimensiÃ³n de salida.
    pub fn output_dim(&self) -> usize {
        self.output_dim
    }

    /// Inferencia stub: produce un vector de ceros de `output_dim`.
    pub fn infer(&self, _input: &[f32]) -> Result<Vec<f32>, String> {
        Ok(vec![0.0; self.output_dim])
    }

    /// Inferencia lineal: dot product + bias.
    pub fn infer_linear(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        if input.len() != self.weights.len() {
            return Err(format!(
                "dimension de entrada {} != {} (pesos)",
                input.len(),
                self.weights.len()
            ));
        }
        let mut out = vec![0.0; self.output_dim];
        let dot: f32 = input
            .iter()
            .zip(self.weights.iter())
            .map(|(a, b)| a * b)
            .sum();
        out.fill(dot + self.bias);
        Ok(out)
    }

    /// Inferencia con normalizaciÃ³n L2 (stub).
    pub fn infer_normalized(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        let mut out = self.infer(input)?;
        let norm: f32 = out.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut out {
                *v /= norm;
            }
        }
        Ok(out)
    }

    /// Inferencia lineal con normalizaciÃ³n L2.
    pub fn infer_linear_normalized(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        let mut out = self.infer_linear(input)?;
        let norm: f32 = out.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut out {
                *v /= norm;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0111-01
    fn test_ac_0111_01_onnx_load_safe() {
        let model = OnnxModel::load("modelo.onnx").expect("load");
        assert_eq!(model.path(), "modelo.onnx");
        assert_eq!(model.output_dim(), 4);
        let out = model.infer(&[1.0, 2.0]).expect("infer stub");
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
}
