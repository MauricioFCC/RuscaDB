//! Tests de aceptación de la feature `rcu` (SPEC-0146).

#![allow(clippy::expect_used)]

use ruscadb::RcuDomain;

#[test] // @spec AC-0146-01
fn test_ac_0146_01_rcu_snapshot() {
    let domain = RcuDomain::new();
    let epoch = domain.enter();
    assert_eq!(epoch, 0);
    domain.exit();
}
