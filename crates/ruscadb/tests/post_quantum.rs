//! Aceptación de ML-KEM (FIPS 203) desde la fachada (SPEC-0180).

#![allow(missing_docs)]
#![cfg(feature = "post-quantum")]
#![allow(clippy::expect_used)]

use ruscadb::MlKem;

/// AC-0180-01 — encapsulate/decapsulate comparten el mismo secreto.
#[test] // @spec AC-0180-01
fn test_ac_0180_01_shared_secret() {
    let keypair = MlKem::keygen().expect("keygen");
    let (ciphertext, sender) = MlKem::encapsulate(keypair.public_key()).expect("encapsulate");
    let receiver = MlKem::decapsulate(keypair.private_key(), &ciphertext).expect("decapsulate");
    assert_eq!(sender, receiver);
    assert_eq!(sender.len(), 32);
}

/// AC-0180-02 — FIPS 203 implicit rejection: tamper da otro secreto.
#[test] // @spec AC-0180-02
fn test_ac_0180_02_tamper() {
    let keypair = MlKem::keygen().expect("keygen");
    let (mut ciphertext, sender) = MlKem::encapsulate(keypair.public_key()).expect("encapsulate");
    ciphertext[0] ^= 0x01;
    let receiver =
        MlKem::decapsulate(keypair.private_key(), &ciphertext).expect("decapsulate tampered");
    assert_ne!(
        sender, receiver,
        "implicit rejection debe cambiar el secreto"
    );
}
