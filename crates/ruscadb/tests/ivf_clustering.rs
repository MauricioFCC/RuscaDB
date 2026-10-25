//! Tests de aceptación de la feature `ivf-clustering` (SPEC-0147).

#![allow(clippy::expect_used)]

use ruscadb::IvfClusterer;

#[test] // @spec AC-0147-01
fn test_ac_0147_01_ivf_cluster() {
    let clusterer = IvfClusterer::new(4);
    let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let assignments = clusterer.cluster(&vectors);
    assert_eq!(assignments.len(), 2);
}
