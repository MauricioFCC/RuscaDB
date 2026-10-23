//! Tests de aceptación de la feature `columnar` (SPEC-0110).

#![allow(clippy::expect_used)]

use ruscadb::ColumnValue;
use ruscadb::ColumnarTable;

#[test] // @spec AC-0110-01
fn test_ac_0110_01_columnar_append_scan() {
    let mut table = ColumnarTable::new(vec!["a".to_string(), "b".to_string()]);
    table
        .append(vec![
            ColumnValue::Int(1),
            ColumnValue::Text("x".to_string()),
        ])
        .expect("append");
    table
        .append(vec![
            ColumnValue::Int(2),
            ColumnValue::Text("y".to_string()),
        ])
        .expect("append");
    assert_eq!(table.len(), 2);
    assert_eq!(table.scan().len(), 2);
    let projected = table.project(&["a"]).expect("project");
    assert_eq!(
        projected,
        vec![vec![ColumnValue::Int(1)], vec![ColumnValue::Int(2)]]
    );
    let filtered = table.filter_eq("a", &ColumnValue::Int(1)).expect("filter");
    assert_eq!(filtered.len(), 1);
}
