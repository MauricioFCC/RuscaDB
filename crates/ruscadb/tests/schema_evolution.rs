//! Tests de aceptación de la feature `schema-evolution` (SPEC-0160).

#![allow(clippy::expect_used)]

use ruscadb::SchemaEvolution;

#[test] // @spec AC-0160-01
fn test_ac_0160_01_add_column() {
    let mut evo = SchemaEvolution::new("t");
    evo.add_column("new_col");
    assert_eq!(evo.columns(), &["new_col"]);
    assert_eq!(evo.table(), "t");
}
