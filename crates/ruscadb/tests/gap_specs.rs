//! Scaffold RED de las specs de mejora SPEC-0194..0198.
//!
//! Los tests están `#[ignore]` porque las specs están `proposed`; `cargo xtask
//! trace` reconoce los nombres y `cargo xtask audit` no los cuenta como huecos.
//! Al implementar cada spec se quita el `#[ignore]` y se sustituye el `panic!`
//! por la aserción real.

#![allow(missing_docs)]
#![allow(clippy::panic)]

use std::collections::BTreeSet;

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// AC-0194-04 — equivalencia set vs oráculo Vec en N/2N/4N.
#[test] // @spec AC-0194-04
fn test_ac_0194_04_equivalence_gate() {
    for n in [1_000usize, 2_000, 4_000] {
        let reclaimed: BTreeSet<u64> = (0..n as u64).filter(|value| *value % 3 == 0).collect();
        let mut got: Vec<u64> = (0..n as u64).collect();
        got.retain(|value| !reclaimed.contains(value));
        let expected: Vec<u64> = (0..n as u64).filter(|value| *value % 3 != 0).collect();
        assert_eq!(got, expected);
    }
}

// SPEC-0196 — conformance multi-binding.

/// AC-0196-01 — Python ejecuta el fixture y coincide con Rust.
#[test]
#[ignore = "SPEC-0196 propuesta: pendiente de implementación"]
// @spec AC-0196-01
fn test_ac_0196_01_python_conformance() {
    panic!("SPEC-0196 pendiente: conformance Python");
}

/// AC-0196-02 — Node ejecuta el fixture y coincide con Rust.
#[test]
#[ignore = "SPEC-0196 propuesta: pendiente de implementación"]
// @spec AC-0196-02
fn test_ac_0196_02_node_conformance() {
    panic!("SPEC-0196 pendiente: conformance Node");
}

/// AC-0196-03 — WASM host ejecuta el fixture y coincide con Rust.
#[test]
#[ignore = "SPEC-0196 propuesta: pendiente de implementación"]
// @spec AC-0196-03
fn test_ac_0196_03_wasm_conformance() {
    panic!("SPEC-0196 pendiente: conformance WASM");
}

/// AC-0196-04 — un hash divergente falla con binding y caso.
#[test]
#[ignore = "SPEC-0196 propuesta: pendiente de implementación"]
// @spec AC-0196-04
fn test_ac_0196_04_hash_mismatch() {
    panic!("SPEC-0196 pendiente: mismatch de hash");
}

// SPEC-0198 — benchmarks macro.

/// AC-0198-03 — workload SQL canónico (scan/filter/group/join/order/limit).
#[test] // @spec AC-0198-03
fn test_ac_0198_03_sql_benchmark() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("sql-workload.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "t",
            vec![
                ColumnDef {
                    name: "g".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "x".to_string(),
                    col_type: ColumnType::Int,
                },
            ],
        )
        .expect("create t");
    for (g, x) in [(1i64, 1i64), (1, 2), (2, 3)] {
        database
            .insert(
                "t",
                ScalarMap::from([
                    ("g".to_string(), ScalarValue::Int(g)),
                    ("x".to_string(), ScalarValue::Int(x)),
                ]),
            )
            .expect("insert t");
    }
    let filtered = database
        .execute("SELECT x FROM t WHERE x >= 2 ORDER BY x DESC LIMIT 2")
        .expect("filter/order/limit");
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].get("x"), Some(&ScalarValue::Int(3)));

    let grouped = database
        .execute("SELECT g, SUM(x) AS total FROM t GROUP BY g ORDER BY g")
        .expect("group");
    assert_eq!(grouped.len(), 2);
    assert_eq!(grouped[0].get("total"), Some(&ScalarValue::Int(3)));
    assert_eq!(grouped[1].get("total"), Some(&ScalarValue::Int(3)));

    database
        .create_table(
            "u",
            vec![
                ColumnDef {
                    name: "g".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "label".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create u");
    for (g, label) in [(1i64, "a"), (2, "b")] {
        database
            .insert(
                "u",
                ScalarMap::from([
                    ("g".to_string(), ScalarValue::Int(g)),
                    ("label".to_string(), ScalarValue::Text(label.to_string())),
                ]),
            )
            .expect("insert u");
    }
    let joined = database
        .execute("SELECT t.x, u.label FROM t JOIN u ON t.g = u.g ORDER BY t.x")
        .expect("join");
    assert_eq!(joined.len(), 3);
    assert_eq!(
        joined[0].get("u.label"),
        Some(&ScalarValue::Text("a".to_string()))
    );
}

/// AC-0198-04 — gate de regresión base/head.
#[test]
#[ignore = "SPEC-0198 propuesta: pendiente de implementación"]
// @spec AC-0198-04
fn test_ac_0198_04_regression_gate() {
    panic!("SPEC-0198 pendiente: gate de regresión");
}
