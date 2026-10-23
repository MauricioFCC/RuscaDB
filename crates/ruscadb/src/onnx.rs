//! Inferencia ONNX opcional (SPEC-0111): stub sin dependencias externas.
//!
//! La integración real con `ort` queda como trabajo futuro cuando se requiera
//! inferencia transformer; este módulo valida la API y los tests sin pulls.

/// Modelo ONNX (stub): carga y produce un vector.
#[derive(Debug, Clone)]
pub struct OnnxModel {
    /// Ruta del fichero del modelo (no se valida aquí).
    path: String,
    /// Dimensión de salida declarada.
    output_dim: usize,
}

impl OnnxModel {
    /// Carga un modelo desde `path` (stub: no valida el fichero).
    pub fn load(path: impl Into<String>) -> Result<Self, String> {
        Ok(Self {
            path: path.into(),
            output_dim: 4,
        })
    }

    /// Carga un modelo con dimensión de salida explícita.
    pub fn load_with_dim(path: impl Into<String>, output_dim: usize) -> Result<Self, String> {
        Ok(Self {
            path: path.into(),
            output_dim,
        })
    }

    /// Ruta del modelo.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Dimensión de salida.
    pub fn output_dim(&self) -> usize {
        self.output_dim
    }

    /// Inferencia stub: produce un vector de ceros de `output_dim`.
    pub fn infer(&self, _input: &[f32]) -> Result<Vec<f32>, String> {
        Ok(vec![0.0; self.output_dim])
    }

    /// Inferencia con normalización L2 (stub).
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
}
