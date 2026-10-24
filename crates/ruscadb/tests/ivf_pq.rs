//! Tests de aceptación de la feature `ivf-pq` (SPEC-0125).

#![allow(clippy::expect_used)]

use ruscadb::IvfPqIndex;

#[test] // @spec AC-0125-01
fn test_ac_0125_01_ivf_pq_quantize() {
    let index = IvfPqIndex::new(4, 2);
    let vector = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let code = index.quantize(&vector);
    assert_eq!(code.len(), 4);
    let mut idx = IvfPqIndex::new(4, 2);
    idx.add(&vector);
    assert_eq!(idx.len(), 1);
}
