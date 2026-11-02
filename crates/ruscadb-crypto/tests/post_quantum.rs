//! Aceptación de ML-KEM FIPS 203, híbrido y keystore (SPEC-0180).

#![allow(missing_docs)]
#![cfg(feature = "post-quantum")]
#![allow(clippy::expect_used)]

use ruscadb_crypto::keystore::Keystore;
use ruscadb_crypto::{HybridKem, MlKem};
use serde_json::Value;

/// AC-0180-03 — vectores ACVP/FIPS 203 de ML-KEM-768.
#[test] // @spec AC-0180-03
fn test_ac_0180_03_acvp_kat() {
    let raw = include_str!("fixtures/mlkem768_kat.json");
    let vector: Value = serde_json::from_str(raw).expect("fixture JSON");
    let encapsulation = &vector["encapsulation"];
    let decapsulation = &vector["decapsulation"];

    let ek = hex::decode(encapsulation["ek"].as_str().expect("ek")).expect("ek hex");
    let m: [u8; 32] = hex::decode(encapsulation["m"].as_str().expect("m"))
        .expect("m hex")
        .try_into()
        .expect("m 32 bytes");
    let expected_c = encapsulation["c"].as_str().expect("c").to_ascii_lowercase();
    let expected_k = encapsulation["k"].as_str().expect("k").to_ascii_lowercase();

    let (ciphertext, shared) = MlKem::encapsulate_deterministic(&ek, m).expect("encapsulate KAT");
    assert_eq!(hex::encode(&ciphertext), expected_c);
    assert_eq!(hex::encode(shared), expected_k);

    let dk = hex::decode(decapsulation["dk"].as_str().expect("dk")).expect("dk hex");
    let c = hex::decode(decapsulation["c"].as_str().expect("c")).expect("c hex");
    let expected_decapsulated = decapsulation["k"].as_str().expect("k").to_ascii_lowercase();
    let decapsulated = MlKem::decapsulate(&dk, &c).expect("decapsulate KAT");
    assert_eq!(hex::encode(decapsulated), expected_decapsulated);
}

/// AC-0180-04 — híbrido X25519+ML-KEM comparte el mismo secreto.
#[test] // @spec AC-0180-04
fn test_ac_0180_04_hybrid_shared_secret() {
    let pair = HybridKem::keygen().expect("hybrid keygen");
    let public = pair.public_key();
    let secret = pair.secret_key();
    let (mut ciphertext, sender) = HybridKem::encapsulate(&public).expect("hybrid encapsulate");
    let receiver = HybridKem::decapsulate(&secret, &ciphertext).expect("hybrid decapsulate");
    assert_eq!(sender, receiver);

    // Tamper del componente X25519 (último bloque) cambia el secreto.
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0x01;
    let tampered = HybridKem::decapsulate(&secret, &ciphertext).expect("tampered");
    assert_ne!(sender, tampered);
}

/// AC-0180-05 — el keystore persiste y recupera claves ML-KEM.
#[test] // @spec AC-0180-05
fn test_ac_0180_05_keystore_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("pq.keystore");
    let pair = MlKem::keygen().expect("keygen");
    {
        let mut keystore = Keystore::load_or_create(&path).expect("keystore");
        keystore.store_ml_kem_keypair("pq1", &pair).expect("store");
        let loaded = keystore
            .load_ml_kem_keypair("pq1")
            .expect("load")
            .expect("presente");
        assert_eq!(loaded, pair);
    }
    let reopened = Keystore::load_or_create(&path).expect("reopen");
    let loaded = reopened
        .load_ml_kem_keypair("pq1")
        .expect("load reopen")
        .expect("presente");
    assert_eq!(loaded, pair);
}
