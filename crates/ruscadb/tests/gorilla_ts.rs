#![allow(missing_docs)]
#![cfg(feature = "gorilla-ts")]
use ruscadb::GorillaTs;

#[test] // @spec AC-0178-01
fn test_ac_0178_01_ratio() {
    let codec = GorillaTs::new();
    let packed = codec.encode(&vec![7.25f64; 1024]);
    assert!(packed.len() < 1024, "constante: {} bytes", packed.len());
}

#[test] // @spec AC-0178-02
fn test_ac_0178_02_lossless() {
    let codec = GorillaTs::new();
    let values = vec![0.0f64, -0.0, 1.5, -2.5e10, f64::NAN, f64::INFINITY];
    let restored = codec.decode(&codec.encode(&values));
    assert_eq!(
        restored.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        values.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
}
