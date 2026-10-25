//! Tests de aceptación de la feature `lance-format` (SPEC-0136).

#![allow(clippy::expect_used)]

use ruscadb::LanceTable;

#[test] // @spec AC-0136-01
fn test_ac_0136_01_lance_append_scan() {
    let mut table = LanceTable::new("test");
    table.append();
    table.append();
    assert_eq!(table.len(), 2);
}
