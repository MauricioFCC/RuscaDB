#![allow(missing_docs)]
#![cfg(feature = "rabitq")]
#![allow(clippy::expect_used)]
use ruscadb::RabitqQuantizer;
#[test] // @spec AC-0171-01
fn test_ac_0171_01_encode() {
    let q = RabitqQuantizer::new();
    let e = q.encode(&[1.0, -2.0, 3.0]);
    assert_eq!(e, vec![1, 0, 1]);
}
#[test] // @spec AC-0171-02
fn test_ac_0171_02_recall() {
    let q = RabitqQuantizer::new();
    assert!(q.estimate_distance(&[1, 0, 1], &[1, 0, 1]) < 1.0);
}
