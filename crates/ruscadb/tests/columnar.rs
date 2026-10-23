//! Tests de aceptación de la feature `columnar` (SPEC-0110).

#![allow(clippy::expect_used)]

use ruscadb::ColumnarTable;

#[test] // @spec AC-0110-01
fn test_ac_0110_01_columnar_append_scan() {
    let mut table = ColumnarTable::new(vec!["a".to_string(), "b".to_string()]);
    table.append(vec!["1".to_string(), "x".to_string()]).expect("append");
    assert_eq!(table.scan().len(), 1);
}
