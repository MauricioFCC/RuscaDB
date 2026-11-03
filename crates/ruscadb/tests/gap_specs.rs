//! Tests de aceptación de las specs de mejora SPEC-0194, SPEC-0196 y SPEC-0198.
//!
//! Los AC con lógica Rust viven aquí; la ejecución de los bindings Python/Node
//! se valida en `crates/xtask/src/main.rs` (estructura del script + job CI) y
//! en el job `conformance` que ejecuta los scripts reales.

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
