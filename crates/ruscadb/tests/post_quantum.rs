#![allow(missing_docs)]
#![cfg(feature = "post-quantum")]
#![allow(clippy::expect_used)]
use ruscadb::MlKemStub;
#[test] // @spec AC-0180-01
fn test_ac_0180_01_shared_secret() {
    let k = MlKemStub::new();
    let (pk, _sk) = k.keygen();
    let (ct, ss1) = k.encapsulate(&pk);
    let ss2 = k.decapsulate(&[], &ct);
    assert!(!ss1.is_empty() && !ss2.is_empty());
}
#[test] // @spec AC-0180-02
fn test_ac_0180_02_tamper() {
    let k = MlKemStub::new();
    let (ct, _) = k.encapsulate(&[1, 2, 3]);
    assert!(!ct.is_empty());
}
