//! Gate de regresión de benchmarks (SPEC-0198).
//!
//! Compara un JSON `base` y otro `head` con la forma:
//!
//! ```json
//! {"benchmarks":[{"name":"knn","qps":1000.0,"p95_ms":1.2}]}
//! ```
//!
//! Falla si el QPS cae más de `max_qps_drop_pct` o si p95 sube más de
//! `max_p95_increase_pct`. Es determinista y no ejecuta benchmarks: solo lee
//! los reportes.

use std::collections::BTreeMap;

use serde_json::Value;

/// Umbrales del gate.
#[derive(Debug, Clone, Copy)]
pub struct BenchThresholds {
    /// Caída máxima permitida de QPS en porcentaje.
    pub max_qps_drop_pct: f64,
    /// Aumento máximo permitido de p95 en porcentaje.
    pub max_p95_increase_pct: f64,
}

impl Default for BenchThresholds {
    fn default() -> Self {
        Self {
            max_qps_drop_pct: 5.0,
            max_p95_increase_pct: 10.0,
        }
    }
}

/// Métrica de un benchmark.
#[derive(Debug, Clone, Copy)]
pub struct BenchMetric {
    /// Queries por segundo.
    pub qps: f64,
    /// Latencia p95 en milisegundos.
    pub p95_ms: f64,
}

/// Evalúa la regresión entre `baseline` y `head`.
///
/// Args:
///     baseline: JSON del run base.
///     head: JSON del run candidato.
///     thresholds: Umbrales permitidos.
///
/// Returns:
///     Texto `[OK] ...` si no hay regresión.
///
/// Errors:
///     `[FAIL] ...` con el benchmark y la métrica que incumple.
pub fn evaluate(
    baseline: &Value,
    head: &Value,
    thresholds: &BenchThresholds,
) -> Result<String, String> {
    let baseline = metrics(baseline)?;
    let head = metrics(head)?;
    let mut failures = Vec::new();
    for (name, candidate) in &head {
        let Some(base) = baseline.get(name) else {
            failures.push(format!("benchmark '{name}' ausente en baseline"));
            continue;
        };
        if base.qps > 0.0 {
            let floor = base.qps * (1.0 - thresholds.max_qps_drop_pct / 100.0);
            if candidate.qps < floor {
                failures.push(format!(
                    "{name}: QPS {:.2} < piso {:.2} (base {:.2}, caída > {:.1}%)",
                    candidate.qps, floor, base.qps, thresholds.max_qps_drop_pct
                ));
            }
        }
        if base.p95_ms > 0.0 {
            let ceiling = base.p95_ms * (1.0 + thresholds.max_p95_increase_pct / 100.0);
            if candidate.p95_ms > ceiling {
                failures.push(format!(
                    "{name}: p95 {:.2}ms > techo {:.2}ms (base {:.2}ms, subida > {:.1}%)",
                    candidate.p95_ms, ceiling, base.p95_ms, thresholds.max_p95_increase_pct
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(format!(
            "[OK] bench-gate: {} benchmarks sin regresión",
            head.len()
        ))
    } else {
        Err(format!(
            "[FAIL] bench-gate:\n  - {}",
            failures.join("\n  - ")
        ))
    }
}

/// Extrae `name -> BenchMetric` del JSON.
fn metrics(value: &Value) -> Result<BTreeMap<String, BenchMetric>, String> {
    let array = value
        .get("benchmarks")
        .and_then(Value::as_array)
        .ok_or_else(|| "JSON sin 'benchmarks' array".to_string())?;
    let mut out = BTreeMap::new();
    for item in array {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "benchmark sin 'name'".to_string())?;
        let qps = item
            .get("qps")
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("benchmark '{name}' sin 'qps' numérico"))?;
        let p95_ms = item
            .get("p95_ms")
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("benchmark '{name}' sin 'p95_ms' numérico"))?;
        out.insert(name.to_string(), BenchMetric { qps, p95_ms });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Reporte con un único benchmark.
    fn report(qps: f64, p95_ms: f64) -> Value {
        json!({ "benchmarks": [{ "name": "knn", "qps": qps, "p95_ms": p95_ms }] })
    }

    /// AC-0198-04 — el gate acepta head igual y falla ante caída de QPS,
    /// subida de p95 o benchmark ausente en base.
    #[test] // @spec AC-0198-04
    fn test_ac_0198_04_regression_gate() {
        let thresholds = BenchThresholds::default();

        let ok = evaluate(&report(1000.0, 1.0), &report(1000.0, 1.0), &thresholds)
            .expect("sin regresión");
        assert!(ok.contains("[OK]"), "{ok}");

        let qps_error = evaluate(&report(1000.0, 1.0), &report(900.0, 1.0), &thresholds)
            .expect_err("caída de QPS");
        assert!(
            qps_error.contains("knn") && qps_error.contains("QPS"),
            "{qps_error}"
        );

        let p95_error = evaluate(&report(1000.0, 1.0), &report(1000.0, 2.0), &thresholds)
            .expect_err("subida de p95");
        assert!(
            p95_error.contains("knn") && p95_error.contains("p95"),
            "{p95_error}"
        );

        let missing = json!({ "benchmarks": [{ "name": "sql", "qps": 10.0, "p95_ms": 1.0 }] });
        let missing_error =
            evaluate(&report(1000.0, 1.0), &missing, &thresholds).expect_err("benchmark ausente");
        assert!(missing_error.contains("sql"), "{missing_error}");

        assert!(evaluate(&json!({}), &report(1.0, 1.0), &thresholds).is_err());
        assert!(
            evaluate(
                &json!({ "benchmarks": [{ "name": "knn" }] }),
                &report(1.0, 1.0),
                &thresholds
            )
            .is_err()
        );
    }

    /// El umbral es configurable: una caída del 15 % pasa con límite 20 %.
    #[test]
    fn benchmark_thresholds_are_configurable() {
        let thresholds = BenchThresholds {
            max_qps_drop_pct: 20.0,
            max_p95_increase_pct: 50.0,
        };
        assert!(evaluate(&report(1000.0, 1.0), &report(900.0, 1.4), &thresholds).is_ok());
    }
}
