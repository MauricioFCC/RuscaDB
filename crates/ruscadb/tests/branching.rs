//! Tests de aceptacion de la feature `branching` (SPEC-0139).

#![cfg(feature = "branching")]
#![allow(clippy::expect_used)]

use ruscadb::Branch;

#[test] // @spec AC-0139-01
fn test_ac_0139_01_branch_fork() {
    let branch = Branch::fork("feature", 42);
    assert_eq!(branch.name(), "feature");
    assert_eq!(branch.fork_tx(), 42);

    // Un fork de la misma transaccion es independiente del original.
    let copy = Branch::fork("feature", 42);
    assert_eq!(branch, copy);
}
