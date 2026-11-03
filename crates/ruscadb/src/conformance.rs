//! Conformance canónica multi-binding (SPEC-0196).
//!
//! Define el contrato de comparación que replican los scripts Python/Node:
//! filas ordenadas por su JSON canónico y valores `ScalarValue` normalizados a
//! JSON plano. Un mismatch devuelve `FAIL <binding>/<caso>` para que el gate
//! nombre el binding y el caso divergentes.
//!
//! Los números siguen la idea de RFC 8785 (JCS) para flotantes integrales:
//! `2.0` se serializa como `2`, de modo que Python (`2.0`), Node (`2`) y Rust
//! coincidan. Los flotantes no finitos se representan como `null` porque el
//! transporte JSON ya no distingue `NaN`/`Infinity`.

use ruscadb_core::ScalarValue;
use serde_json::{Value, json};

use crate::executor::Row;

/// Rango de enteros exactos en un `f64` (IEEE-754, 2^53).
const MAX_EXACT_INT_F64: f64 = 9_007_199_254_740_992.0;

/// Normaliza un `f64` a número JSON canónico (enteros sin `.0`).
///
/// Args:
///     number: Flotante a normalizar.
///
/// Returns:
///     `Value::Number` entero si el flotante es integral y exacto;
///     `Value::Number` flotante en otro caso; `Value::Null` si no es finito.
pub fn canonical_number(number: f64) -> Value {
    if !number.is_finite() {
        return Value::Null;
    }
    if number.fract() == 0.0 && number.abs() <= MAX_EXACT_INT_F64 {
        // `as i64` es seguro: |number| <= 2^53 < i64::MAX y fract() == 0.
        return Value::Number(serde_json::Number::from(number as i64));
    }
    serde_json::Number::from_f64(number).map_or(Value::Null, Value::Number)
}

/// Normaliza un `ScalarValue` a JSON plano.
pub fn scalar_json(value: &ScalarValue) -> Value {
    match value {
        ScalarValue::Null => Value::Null,
        ScalarValue::Bool(flag) => json!(flag),
        ScalarValue::Int(number) => json!(number),
        ScalarValue::UInt(number) => json!(number),
        ScalarValue::Float(number) => canonical_number(*number),
        ScalarValue::Text(text) => json!(text),
        ScalarValue::Bytes(bytes) => json!(bytes),
        ScalarValue::TimestampMillis(millis) => json!(millis),
    }
}

/// Serializa filas de forma canónica (claves ordenadas y filas ordenadas).
///
/// Returns:
///     JSON compacto con las filas ordenadas por su forma canónica.
///
/// Errors:
///     Texto accionable si un valor no se puede serializar (no debería ocurrir
///     con `serde_json::Value`).
pub fn canonical_rows(rows: &[Row]) -> Result<String, String> {
    let mut keyed: Vec<(String, Value)> = Vec::with_capacity(rows.len());
    for row in rows {
        let mut object = serde_json::Map::new();
        for (key, value) in row {
            object.insert(key.clone(), scalar_json(value));
        }
        let value = Value::Object(object);
        let key = serde_json::to_string(&value)
            .map_err(|error| format!("fila no serializable: {error}"))?;
        keyed.push((key, value));
    }
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    let normalized: Vec<Value> = keyed.into_iter().map(|(_, value)| value).collect();
    serde_json::to_string(&normalized).map_err(|error| format!("JSON canónico: {error}"))
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
    let got = canonical_rows(got)?;
    let expected = canonical_rows(expected)?;
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

    /// AC-0196-05 — el fixture tipado (Float/Text/Bool/Null + error) queda
    /// cubierto y la normalización JCS hace el hash independiente del orden.
    #[test] // @spec AC-0196-05
    fn test_ac_0196_05_typed_fixture() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/conformance/basic.json")).expect("fixture");
        let cases = fixture["cases"].as_array().expect("cases");
        let names: Vec<&str> = cases
            .iter()
            .filter_map(|case| case["name"].as_str())
            .collect();
        for required in ["typed_rows", "float_projection", "missing_column"] {
            assert!(names.contains(&required), "fixture sin caso '{required}'");
        }

        let first = Row::from([
            ("f".to_string(), ScalarValue::Float(1.5)),
            ("s".to_string(), ScalarValue::Text("uno".to_string())),
            ("b".to_string(), ScalarValue::Bool(false)),
            ("n".to_string(), ScalarValue::Null),
        ]);
        let second = Row::from([
            ("f".to_string(), ScalarValue::Float(2.0)),
            ("s".to_string(), ScalarValue::Text("dos".to_string())),
            ("b".to_string(), ScalarValue::Bool(true)),
            ("n".to_string(), ScalarValue::Null),
        ]);

        let forward = canonical_rows(&[first.clone(), second.clone()]).expect("canonical");
        let reverse = canonical_rows(&[second, first]).expect("canonical");
        assert_eq!(forward, reverse, "orden de filas no canónico");
        assert!(
            forward.contains("{\"b\":true,\"f\":2,\"n\":null,\"s\":\"dos\"}"),
            "2.0 no se normalizó a entero: {forward}"
        );
        assert_eq!(canonical_number(2.0), json!(2));
        assert_eq!(canonical_number(-0.0), json!(0));
        assert_eq!(canonical_number(1.5), json!(1.5));
        assert_eq!(canonical_number(f64::NAN), Value::Null);
    }
}
