//! Tests de aceptación de la feature `prepared` (SPEC-0148).

#![allow(clippy::expect_used)]

use ruscadb::PreparedStatement;

#[test] // @spec AC-0148-01
fn test_ac_0148_01_prepared_select() {
    let stmt = PreparedStatement::prepare("SELECT * FROM t");
    assert_eq!(stmt.query(), "SELECT * FROM t");
}
