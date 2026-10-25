//! Tests de aceptación de la feature `sql-std` (SPEC-0141).

#![allow(clippy::expect_used)]

use ruscadb::WindowFunc;

#[test] // @spec AC-0141-01
fn test_ac_0141_01_row_number() {
    let values = vec![10, 20, 30];
    let rn = WindowFunc::row_number(&values);
    assert_eq!(rn, vec![1, 2, 3]);
}
