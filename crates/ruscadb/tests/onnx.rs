//! Tests de aceptación de la feature `onnx` (SPEC-0111).

#![allow(clippy::expect_used)]

use ruscadb::OnnxModel;

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
