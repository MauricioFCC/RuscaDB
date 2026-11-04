//! Scaffold RED de specs propuestas (SPEC-0201/0202).
//!
//! Los tests están `#[ignore]` porque las specs están `proposed`; `cargo xtask
//! trace` reconoce los nombres y `audit` no los cuenta como huecos. Al
//! implementar cada spec se quita el `#[ignore]` y se sustituye el `panic!`.

#![allow(missing_docs)]
#![allow(clippy::panic)]

/// AC-0201-01 — ANALYZE calcula rows/nulls/distinct/min/max contra oráculo.
#[test]
#[ignore = "SPEC-0201 propuesta: pendiente de implementación"]
// @spec AC-0201-01
fn test_ac_0201_01_stats_collect() {
    panic!("SPEC-0201 pendiente: estadísticas de tabla");
}

/// AC-0201-02 — selectividad de igualdad/rango con error ≤ 10 %.
#[test]
#[ignore = "SPEC-0201 propuesta: pendiente de implementación"]
// @spec AC-0201-02
fn test_ac_0201_02_selectivity_estimate() {
    panic!("SPEC-0201 pendiente: estimación de selectividad");
}

/// AC-0201-03 — IndexScan para igualdad selectiva y SeqScan para el resto.
#[test]
#[ignore = "SPEC-0201 propuesta: pendiente de implementación"]
// @spec AC-0201-03
fn test_ac_0201_03_plan_choice() {
    panic!("SPEC-0201 pendiente: elección de plan por coste");
}

/// AC-0201-04 — IndexNestedLoop vs HashJoin elegido por coste y visible en EXPLAIN.
#[test]
#[ignore = "SPEC-0201 propuesta: pendiente de implementación"]
// @spec AC-0201-04
fn test_ac_0201_04_join_strategy_cost() {
    panic!("SPEC-0201 pendiente: coste de join");
}

/// AC-0202-01 — 401 sin token / token erróneo; 200 con token correcto.
#[test]
#[ignore = "SPEC-0202 propuesta: pendiente de implementación"]
// @spec AC-0202-01
fn test_ac_0202_01_metrics_bearer_auth() {
    panic!("SPEC-0202 pendiente: auth Bearer");
}

/// AC-0202-02 — comparación de token en tiempo constante y solo igual ⇒ true.
#[test]
#[ignore = "SPEC-0202 propuesta: pendiente de implementación"]
// @spec AC-0202-02
fn test_ac_0202_02_constant_time_compare() {
    panic!("SPEC-0202 pendiente: comparación constante");
}

/// AC-0202-03 — handshake TLS real con certificado autofirmado.
#[test]
#[ignore = "SPEC-0202 propuesta: pendiente de implementación"]
// @spec AC-0202-03
fn test_ac_0202_03_metrics_tls_handshake() {
    panic!("SPEC-0202 pendiente: handshake TLS");
}

/// AC-0202-04 — TLS incompleto/inválido falla al arrancar (fail-closed).
#[test]
#[ignore = "SPEC-0202 propuesta: pendiente de implementación"]
// @spec AC-0202-04
fn test_ac_0202_04_tls_fail_closed() {
    panic!("SPEC-0202 pendiente: fail-closed TLS");
}
