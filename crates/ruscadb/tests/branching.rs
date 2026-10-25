//! Tests de aceptación de la feature `branching` (SPEC-0139).

#![allow(clippy::expect_used)]

use ruscadb::Branch;

#[test] // @spec AC-0139-01
fn test_ac_0139_01_branch_fork() {
    let branch = Branch::fork("feature", 42);
    assert_eq!(branch.name(), "feature");
    assert_eq!(branch.fork_tx(), 42);
}
