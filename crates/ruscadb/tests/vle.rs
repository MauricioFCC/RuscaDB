//! Tests de aceptación de la feature `vle` (SPEC-0137).

#![allow(clippy::expect_used)]

use ruscadb::VleDatabase;

#[test] // @spec AC-0137-01
fn test_ac_0137_01_vle_create() {
    let db = VleDatabase::new("test.pvdb");
    assert_eq!(db.path(), "test.pvdb");
}
