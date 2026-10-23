//! Tests de aceptación de la feature `ts-advanced` (SPEC-0118).

#![allow(clippy::expect_used)]

use ruscadb::Downsampler;

#[test] // @spec AC-0118-01
fn test_ac_0118_01_downsample() {
    let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let down = Downsampler::downsample(&values, 2);
    assert_eq!(down.len(), 3);
}
