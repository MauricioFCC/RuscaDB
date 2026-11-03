//! Conformance canónica multi-binding (SPEC-0196).
//!
//! Define el contrato de comparación que replican los scripts Python/Node:
//! filas ordenadas por su JSON canónico y valores `ScalarValue` normalizados a
//! JSON plano. Un mismatch devuelve `FAIL <binding>/<caso>` para que el gate
//! nombre el binding y el caso divergentes.

use ruscadb_core::ScalarValue;
use serde_json::{Value, json};

use crate::executor::Row;

/// Normaliza un `ScalarValue` a JSON plano.
pub fn scalar_json(value: &ScalarValue) -> Value {
    match value {
        ScalarValue::Null => Value::Null,
        ScalarValue::Bool(flag) => json!(flag),
        ScalarValue::Int(number) => json!(number),
        ScalarValue::UInt(number) => json!(number),
        ScalarValue::Float(number) => {
            serde_json::Number::from_f64(*number).map_or(Value::Null, Value::Number)
        }
        ScalarValue::Text(text) => json!(text),
        ScalarValue::Bytes(bytes) => json!(bytes),
        ScalarValue::TimestampMillis(millis) => json!(millis),
    }
}

/// Serializa filas de forma canónica (claves ordenadas y filas ordenadas).
pub fn canonical_rows(rows: &[Row]) -> String {
    let mut normalized: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut object = serde_json::Map::new();
            for (key, value) in row {
                object.insert(key.clone(), scalar_json(value));
            }
            Value::Object(object)
        })
        .collect();
    normalized.sort_by_key(|value| serde_json::to_string(value).unwrap_or_default());
    serde_json::to_string(&normalized).unwrap_or_else(|_| "[]".to_string())
}

/// Compara el resultado de un binding con el esperado.
///
/// Args:
///     binding: Nombre del binding (`python`, `node`, `wasm`).
///     case: Nombre del caso del fixture.
///     got: Filas producidas por el binding.
///     expected: Filas canónicas esperadas.
///
/// Returns:
///     `Ok(())` si coinciden.
///
/// Errors:
///     Texto `FAIL <binding>/<caso>: ...` si divergen.
pub fn compare(binding: &str, case: &str, got: &[Row], expected: &[Row]) -> Result<(), String> {
    let got = canonical_rows(got);
    let expected = canonical_rows(expected);
    if got == expected {
        Ok(())
    } else {
        Err(format!(
            "FAIL {binding}/{case}: got={got} expected={expected}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-0196-04 — un resultado alterado falla nombrando binding y caso.
    #[test] // @spec AC-0196-04
    fn test_ac_0196_04_hash_mismatch() {
        let got = vec![Row::from([("x".to_string(), ScalarValue::Int(2))])];
        let expected = vec![Row::from([("x".to_string(), ScalarValue::Int(1))])];
        let error = compare("python", "filter_gte", &got, &expected).expect_err("mismatch");
        assert!(error.contains("python"), "{error}");
        assert!(error.contains("filter_gte"), "{error}");
        assert!(compare("node", "filter_gte", &got, &got).is_ok());
    }
}
