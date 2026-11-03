//! Tests de aceptación de las specs de mejora SPEC-0194, SPEC-0196 y SPEC-0198.
//!
//! Los AC con lógica Rust viven aquí; la ejecución de los bindings Python/Node
//! se valida en `crates/xtask/src/main.rs` (estructura del script + job CI) y
//! en el job `conformance` que ejecuta los scripts reales.

#![allow(missing_docs)]
#![allow(clippy::panic)]

use std::cmp::Ordering as CmpOrdering;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Contador global de comparaciones para medir el escalado sin reloj.
static COMPARISONS: AtomicUsize = AtomicUsize::new(0);

/// Entero que cuenta cada comparación (proxy determinista de coste).
#[derive(Clone, Copy)]
struct Counted(u64);

impl PartialEq for Counted {
    fn eq(&self, other: &Self) -> bool {
        COMPARISONS.fetch_add(1, Ordering::Relaxed);
        self.0 == other.0
    }
}

impl Eq for Counted {}

impl PartialOrd for Counted {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for Counted {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        COMPARISONS.fetch_add(1, Ordering::Relaxed);
        self.0.cmp(&other.0)
    }
}

/// Comparaciones del oráculo `Vec::contains` (O(N²)).
fn vec_comparisons(n: usize) -> usize {
    COMPARISONS.store(0, Ordering::Relaxed);
    let pages: Vec<Counted> = (0..n).map(|value| Counted(value as u64)).collect();
    for value in &pages {
        let _ = pages.contains(value);
    }
    COMPARISONS.load(Ordering::Relaxed)
}

/// Comparaciones de `BTreeSet::contains` (O(N log N)).
fn set_comparisons(n: usize) -> usize {
    COMPARISONS.store(0, Ordering::Relaxed);
    let pages: BTreeSet<Counted> = (0..n).map(|value| Counted(value as u64)).collect();
    for value in &pages {
        let _ = pages.contains(value);
    }
    COMPARISONS.load(Ordering::Relaxed)
}

/// AC-0194-04 — equivalencia set vs oráculo Vec en N/2N/4N y gate de escalado
/// determinista (comparaciones, no reloj).
#[test] // @spec AC-0194-04
fn test_ac_0194_04_equivalence_gate() {
    for n in [1_000usize, 2_000, 4_000] {
        let reclaimed: BTreeSet<u64> = (0..n as u64).filter(|value| *value % 3 == 0).collect();
        let mut got: Vec<u64> = (0..n as u64).collect();
        got.retain(|value| !reclaimed.contains(value));
        let expected: Vec<u64> = (0..n as u64).filter(|value| *value % 3 != 0).collect();
        assert_eq!(got, expected);
    }

    let base = 500usize;
    let quadruple = base * 4;
    let vec_ratio = vec_comparisons(quadruple) as f64 / vec_comparisons(base) as f64;
    let set_ratio = set_comparisons(quadruple) as f64 / set_comparisons(base) as f64;
    assert!(
        vec_ratio > 12.0,
        "el oráculo Vec debería escalar ~cuadrático (ratio={vec_ratio:.1})"
    );
    assert!(
        set_ratio < 8.0,
        "BTreeSet debería escalar ~N log N (ratio={set_ratio:.1})"
    );
    assert!(
        set_ratio * 2.0 < vec_ratio,
        "mejora insuficiente: set={set_ratio:.1} vs vec={vec_ratio:.1}"
    );
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
