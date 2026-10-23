//! Inferencia ONNX opcional (SPEC-0111): stub sin dependencias externas.
//!
//! La integración real con `ort` queda como trabajo futuro cuando se requiera
//! inferencia transformer; este módulo valida la API y los tests sin pulls.

/// Modelo ONNX (stub): carga y produce un vector vacío.
#[derive(Debug, Clone)]
pub struct OnnxModel {
    /// Ruta del fichero del modelo (no se valida aquí).
    path: String,
}

impl OnnxModel {
    /// Carga un modelo desde `path` (stub: no valida el fichero).
    pub fn load(path: impl Into<String>) -> Result<Self, String> {
        Ok(Self { path: path.into() })
    }

    /// Ruta del modelo.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Inferencia stub: devuelve un vector vacío.
    pub fn infer(&self, _input: &[f32]) -> Result<Vec<f32>, String> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0111-01
    fn test_ac_0111_01_onnx_load_safe() {
        let model = OnnxModel::load("modelo.onnx").expect("load stub");
        assert_eq!(model.path(), "modelo.onnx");
        let out = model.infer(&[1.0, 2.0]).expect("infer stub");
        assert!(out.is_empty());
    }
}
