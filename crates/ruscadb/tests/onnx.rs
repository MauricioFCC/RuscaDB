//! Tests de aceptación de la feature `onnx` (SPEC-0111).

#![allow(clippy::expect_used)]

use ruscadb::OnnxModel;

#[test] // @spec AC-0111-01
fn test_ac_0111_01_onnx_load_safe() {
    let model = OnnxModel::load("modelo.onnx").expect("load");
    assert_eq!(model.path(), "modelo.onnx");
}
