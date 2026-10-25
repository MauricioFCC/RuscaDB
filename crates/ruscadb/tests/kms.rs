//! Tests de aceptación de la feature `kms` (SPEC-0143).

#![allow(clippy::expect_used)]

use ruscadb::KmsProvider;

#[test] // @spec AC-0143-01
fn test_ac_0143_01_kms_roundtrip() {
    let provider = KmsProvider::new(vec![0xAB, 0xCD]);
    let plaintext = b"secret key";
    let wrapped = provider.wrap(plaintext);
    let unwrapped = provider.unwrap(&wrapped);
    assert_eq!(unwrapped, plaintext);
}
