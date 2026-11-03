//! Conformance baseline compartido (SPEC-0190, P2.7; tipado en SPEC-0196-05).

#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, Row, ScalarMap, ScalarValue, canonical_number,
    scalar_json,
};
use serde_json::Value;

/// Aplica la normalización JCS a los números de un JSON esperado.
fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Number(number) => number
            .as_f64()
            .map_or_else(|| value.clone(), canonical_number),
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, item)| (key.clone(), canonical_json(item)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Convierte una fila a JSON canónico usando el contrato de SPEC-0196.
fn row_json(row: &Row) -> Value {
    let mut object = serde_json::Map::new();
    for (key, value) in row {
        object.insert(key.clone(), scalar_json(value));
    }
    Value::Object(object)
}

/// Siembra las tablas `t(x Int)` y `typed(f Float, s Text, b Bool, n Int)`.
fn seed(database: &mut Database) {
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "x".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create t");
    for value in 1..=3i64 {
        database
            .insert(
                "t",
                ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
            )
            .expect("insert t");
    }

    database
        .create_table(
            "typed",
            vec![
                ColumnDef {
                    name: "f".to_string(),
                    col_type: ColumnType::Float,
                },
                ColumnDef {
                    name: "s".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "b".to_string(),
                    col_type: ColumnType::Bool,
                },
                ColumnDef {
                    name: "n".to_string(),
                    col_type: ColumnType::Int,
                },
            ],
        )
        .expect("create typed");
    for (f, s, b) in [(1.5_f64, "uno", false), (2.0_f64, "dos", true)] {
        database
            .insert(
                "typed",
                ScalarMap::from([
                    ("f".to_string(), ScalarValue::Float(f)),
                    ("s".to_string(), ScalarValue::Text(s.to_string())),
                    ("b".to_string(), ScalarValue::Bool(b)),
                    ("n".to_string(), ScalarValue::Null),
                ]),
            )
            .expect("insert typed");
    }
}

#[test] // @spec AC-0190-01
fn test_ac_0190_01_conformance_fixtures() {
    let fixture: Value =
        serde_json::from_str(include_str!("conformance/basic.json")).expect("fixture");
    let cases = fixture["cases"].as_array().expect("cases");

    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("conformance.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    seed(&mut database);

    for case in cases {
        let sql = case["sql"].as_str().expect("sql");
        let name = case["name"].as_str().unwrap_or("?");
        if case.get("expect_error").and_then(Value::as_bool) == Some(true) {
            assert!(
                database.execute(sql).is_err(),
                "caso {name}: se esperaba error para '{sql}'"
            );
            continue;
        }
        let expected = case["expect"].as_array().expect("expect");
        let rows = database.execute(sql).expect(sql);
        let actual: Vec<Value> = rows.iter().map(row_json).collect();
        assert_eq!(
            Value::Array(actual),
            canonical_json(&Value::Array(expected.clone())),
            "caso {name} ({sql})"
        );
    }
}
