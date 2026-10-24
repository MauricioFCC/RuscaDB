//! Tests de aceptación de la feature `arrow-columnar` (SPEC-0124).

#![allow(clippy::expect_used)]

use ruscadb::{ArrowColumnType, ArrowColumnarTable};

#[test] // @spec AC-0124-01
fn test_ac_0124_01_arrow_append_scan() {
    let mut table = ArrowColumnarTable::new(vec![
        ("id".to_string(), ArrowColumnType::Int64),
        ("name".to_string(), ArrowColumnType::Utf8),
    ]);
    table.append_row(vec!["1".to_string(), "alice".to_string()]);
    table.append_row(vec!["2".to_string(), "bob".to_string()]);
    assert_eq!(table.num_rows(), 2);
    table.flush();
    assert_eq!(table.num_batches(), 1);
    let scanned = table.scan();
    assert_eq!(scanned.len(), 2);
    assert_eq!(scanned[0], vec!["1", "alice"]);
}
