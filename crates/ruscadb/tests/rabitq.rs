#![allow(missing_docs)]
#![cfg(feature = "rabitq")]
use ruscadb::RabitqQuantizer;

#[test] // @spec AC-0171-01
fn test_ac_0171_01_encode() {
    let quantizer = RabitqQuantizer::new();
    let code = quantizer.encode(&[1.0, -2.0, 3.0, 4.0, -5.0, 6.0, 7.0, -8.0]);
    assert_eq!(code.dim, 8);
    assert_eq!(code.bytes(), 1);
    assert!(RabitqQuantizer::compression_ratio(128) >= 30.0);
}

#[test] // @spec AC-0171-02
fn test_ac_0171_02_recall() {
    let quantizer = RabitqQuantizer::new();
    // Dos vectores bien separados: el estimador debe ordenarlos igual que L2.
    let a = vec![1.0f32; 32];
    let b = vec![-1.0f32; 32];
    let code_a = quantizer.encode(&a);
    let code_b = quantizer.encode(&b);
    let near_a = quantizer.estimate_l2(&a, &code_a);
    let far_a = quantizer.estimate_l2(&a, &code_b);
    assert!(near_a < far_a, "{near_a} < {far_a}");
}
