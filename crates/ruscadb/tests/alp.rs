#![allow(missing_docs)]
#![cfg(feature = "alp")]
use ruscadb::AlpCompressor;

#[test] // @spec AC-0175-01
fn test_ac_0175_01_ratio() {
    let alp = AlpCompressor::new();
    let values: Vec<f64> = (0..512).map(|i| 10.0 + f64::from(i % 100) * 0.01).collect();
    let packed = alp.encode(&values);
    assert!(alp.is_decimal_encoded(&packed));
    assert!(packed.len() < values.len() * 2, "{} bytes", packed.len());
}

#[test] // @spec AC-0175-02
fn test_ac_0175_02_lossless() {
    let alp = AlpCompressor::new();
    let values = vec![1.23f64, -4.56, 0.0, -0.0, 100.0];
    let restored = alp.decode(&alp.encode(&values));
    assert_eq!(
        restored.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        values.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
}
