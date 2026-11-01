//! Conformance baseline compartido (SPEC-0190, P2.7).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, ScalarValue};
use serde_json::{Value, json};

/// Convierte una fila a JSON canónico.
fn row_json(row: &Row) -> Value {
    let mut object = serde_json::Map::new();
    for (key, value) in row {
        let json_value = match value {
            ScalarValue::Null => Value::Null,
            ScalarValue::Bool(flag) => json!(flag),
            ScalarValue::Int(number) => json!(number),
            ScalarValue::UInt(number) => json!(number),
            ScalarValue::Float(number) => json!(number),
            ScalarValue::Text(text) => json!(text),
            ScalarValue::TimestampMillis(millis) => json!(millis),
            ScalarValue::Bytes(bytes) => json!(format!("{bytes:?}")),
        };
        object.insert(key.clone(), json_value);
    }
    Value::Object(object)
}

#[test] // @spec AC-0190-01
fn test_ac_0190_01_conformance_fixtures() {
    let fixture: Value =
        serde_json::from_str(include_str!("conformance/basic.json")).expect("fixture");
    let cases = fixture["cases"].as_array().expect("cases");

    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("conformance.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "x".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create");
    for value in 1..=3i64 {
        database
            .insert(
                "t",
                ruscadb::ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
            )
            .expect("insert");
    }

    for case in cases {
        let sql = case["sql"].as_str().expect("sql");
        let expected = case["expect"].as_array().expect("expect");
        let rows = database.execute(sql).expect(sql);
        let actual: Vec<Value> = rows.iter().map(row_json).collect();
        assert_eq!(
            Value::Array(actual),
            Value::Array(expected.clone()),
            "caso {} ({sql})",
            case["name"].as_str().unwrap_or("?")
        );
    }
}
