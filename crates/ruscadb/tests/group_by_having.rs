//! GROUP BY multi-clave + HAVING en la fachada (SPEC-0051).
//!
//! Cubre los criterios AC-0051-01..05: agrupación por tupla de claves,
//! filtrado post-agregación con `HAVING`, error sin `GROUP BY`, tabla vacía
//! y no regresión de la agregación de una clave (SPEC-0041).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use proptest::prelude::*;
use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, RuscaError, ScalarMap, ScalarValue};

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `t(a INT, b TEXT, c TEXT)` vacía.
fn create_abc_table(database: &mut Database) {
    database
        .create_table(
            "t",
            vec![
                ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "b".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "c".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Inserta en `t` las filas `(a, b, c)` dadas.
fn seed_triples(database: &mut Database, triples: &[(i64, &str, &str)]) {
    for (value, second, third) in triples {
        let scalars = ScalarMap::from([
            ("a".to_string(), ScalarValue::Int(*value)),
            ("b".to_string(), ScalarValue::Text((*second).to_string())),
            ("c".to_string(), ScalarValue::Text((*third).to_string())),
        ]);
        database.insert("t", scalars).expect("insert");
    }
}

/// Extrae el conteo `count` de una fila agregada.
fn row_count(row: &BTreeMap<String, ScalarValue>) -> i64 {
    match row.get("count") {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en 'count', se obtuvo {other:?}"),
    }
}

/// AC-0051-01 — `GROUP BY a, b` agrupa por tupla de claves.
///
/// Given: filas con dos columnas de agrupación.
/// When: se ejecuta `GROUP BY b, c` con `COUNT`.
/// Then: cada combinación (b, c) forma un grupo con su conteo.
#[test]
// @spec AC-0051-01
fn test_ac_0051_01_multi_key_groups() {
    let (_dir, mut database) = open_test_db("ac0051_01");
    create_abc_table(&mut database);
    seed_triples(
        &mut database,
        &[(1, "x", "p"), (2, "x", "p"), (3, "x", "q"), (4, "y", "p")],
    );

    let rows = database
        .execute("SELECT b, c, COUNT(*) FROM t GROUP BY b, c")
        .expect("select");
    assert_eq!(rows.len(), 3, "tres combinaciones (b, c): {rows:?}");
    let mut counts: BTreeMap<(String, String), i64> = BTreeMap::new();
    for row in &rows {
        let first = match row.get("b") {
            Some(ScalarValue::Text(value)) => value.clone(),
            other => panic!("se esperaba Text en 'b', se obtuvo {other:?}"),
        };
        let second = match row.get("c") {
            Some(ScalarValue::Text(value)) => value.clone(),
            other => panic!("se esperaba Text en 'c', se obtuvo {other:?}"),
        };
        counts.insert((first, second), row_count(row));
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            (("x".to_string(), "p".to_string()), 2),
            (("x".to_string(), "q".to_string()), 1),
            (("y".to_string(), "p".to_string()), 1),
        ])
    );
}

/// AC-0051-02 — `HAVING` filtra grupos después de agregar.
///
/// Given: grupos con conteos distintos.
/// When: se ejecuta `GROUP BY b HAVING COUNT(*) > 1`.
/// Then: solo los grupos con más de una fila sobreviven.
#[test]
// @spec AC-0051-02
fn test_ac_0051_02_having_filters_groups() {
    let (_dir, mut database) = open_test_db("ac0051_02");
    create_abc_table(&mut database);
    seed_triples(
        &mut database,
        &[(1, "x", "p"), (2, "x", "q"), (3, "y", "p"), (4, "z", "p")],
    );

    let rows = database
        .execute("SELECT b, COUNT(*) FROM t GROUP BY b HAVING COUNT(*) > 1")
        .expect("select");
    assert_eq!(rows.len(), 1, "solo el grupo 'x' sobrevive: {rows:?}");
    assert_eq!(rows[0].get("b"), Some(&ScalarValue::Text("x".to_string())));
    assert_eq!(row_count(&rows[0]), 2);
}

/// AC-0051-03 — `HAVING` sin `GROUP BY` es error de parseo accionable.
///
/// Given: una consulta con `HAVING` y sin `GROUP BY`.
/// When: se parsea/ejecuta.
/// Then: devuelve error de parseo accionable.
#[test]
// @spec AC-0051-03
fn test_ac_0051_03_having_without_group_by_errors() {
    let (_dir, mut database) = open_test_db("ac0051_03");
    create_abc_table(&mut database);

    let error = database
        .execute("SELECT COUNT(*) FROM t HAVING COUNT(*) > 1")
        .expect_err("HAVING sin GROUP BY debe fallar");
    assert!(
        matches!(error, RuscaError::ParseError { .. }),
        "se esperaba ParseError, se obtuvo {error:?}"
    );
    assert!(
        error.to_string().contains("GROUP BY"),
        "el mensaje debe mencionar GROUP BY: {error}"
    );
}

/// AC-0051-04 — tabla vacía con `GROUP BY` + `HAVING` devuelve 0 filas.
///
/// Given: una tabla vacía.
/// When: se ejecuta `GROUP BY` con `HAVING`.
/// Then: devuelve 0 filas sin panics.
#[test]
// @spec AC-0051-04
fn test_ac_0051_04_empty_table() {
    let (_dir, mut database) = open_test_db("ac0051_04");
    create_abc_table(&mut database);

    let rows = database
        .execute("SELECT b, COUNT(*) FROM t GROUP BY b HAVING COUNT(*) > 0")
        .expect("select");
    assert_eq!(rows.len(), 0, "tabla vacía implica 0 grupos: {rows:?}");

    let multi = database
        .execute("SELECT b, c, COUNT(*) FROM t GROUP BY b, c HAVING COUNT(*) >= 1")
        .expect("select");
    assert_eq!(multi.len(), 0, "multi-clave vacía implica 0 grupos");
}

/// AC-0051-05 — `GROUP BY` de una sola clave sigue funcionando igual.
///
/// Given: `GROUP BY` de una sola clave (comportamiento SPEC-0041).
/// When: se ejecuta.
/// Then: sin regresión.
#[test]
// @spec AC-0051-05
fn test_ac_0051_05_single_key_unchanged() {
    let (_dir, mut database) = open_test_db("ac0051_05");
    create_abc_table(&mut database);
    seed_triples(
        &mut database,
        &[(1, "x", "p"), (2, "x", "q"), (3, "y", "p"), (4, "z", "p")],
    );

    let rows = database
        .execute("SELECT b, COUNT(*) FROM t GROUP BY b")
        .expect("select");
    assert_eq!(rows.len(), 3);
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for row in &rows {
        let label = match row.get("b") {
            Some(ScalarValue::Text(value)) => value.clone(),
            other => panic!("se esperaba Text en 'b', se obtuvo {other:?}"),
        };
        counts.insert(label, row_count(row));
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            ("x".to_string(), 2),
            ("y".to_string(), 1),
            ("z".to_string(), 1),
        ])
    );
}

/// BVA — `HAVING` que elimina todos los grupos deja 0 filas.
#[test]
fn test_ac_0051_bva_having_removes_all_groups() {
    let (_dir, mut database) = open_test_db("ac0051_bva_none");
    create_abc_table(&mut database);
    seed_triples(&mut database, &[(1, "x", "p"), (2, "y", "q")]);

    let rows = database
        .execute("SELECT b, COUNT(*) FROM t GROUP BY b HAVING COUNT(*) > 100")
        .expect("select");
    assert_eq!(rows.len(), 0, "ningún grupo supera el umbral: {rows:?}");
}

/// BVA — `HAVING` que deja todos los grupos no filtra nada.
#[test]
fn test_ac_0051_bva_having_keeps_all_groups() {
    let (_dir, mut database) = open_test_db("ac0051_bva_all");
    create_abc_table(&mut database);
    seed_triples(&mut database, &[(1, "x", "p"), (2, "y", "q")]);

    let rows = database
        .execute("SELECT b, COUNT(*) FROM t GROUP BY b HAVING COUNT(*) >= 1")
        .expect("select");
    assert_eq!(rows.len(), 2, "todos los grupos sobreviven: {rows:?}");
}

/// BVA — `HAVING` con `SUM`/`AVG` y conjunción `AND` sobre agregados.
#[test]
fn test_ac_0051_bva_having_sum_avg_and() {
    let (_dir, mut database) = open_test_db("ac0051_bva_agg");
    create_abc_table(&mut database);
    seed_triples(
        &mut database,
        &[
            (10, "x", "p"),
            (20, "x", "p"),
            (1, "y", "p"),
            (100, "z", "p"),
        ],
    );

    let rows = database
        .execute("SELECT b, SUM(a) FROM t GROUP BY b HAVING SUM(a) > 5 AND AVG(a) < 50")
        .expect("select");
    assert_eq!(rows.len(), 1, "solo 'x' cumple ambas: {rows:?}");
    assert_eq!(rows[0].get("b"), Some(&ScalarValue::Text("x".to_string())));
    assert_eq!(rows[0].get("sum_a"), Some(&ScalarValue::Int(30)));
}

/// NF-0051-02 — claves inexistentes dan error accionable (sin panic).
#[test]
fn test_ac_0051_bva_unknown_key_is_actionable() {
    let (_dir, mut database) = open_test_db("ac0051_bva_err");
    create_abc_table(&mut database);
    seed_triples(&mut database, &[(1, "x", "p")]);

    let missing_group = database
        .execute("SELECT b, COUNT(*) FROM t GROUP BY ausente")
        .expect_err("GROUP BY inexistente");
    assert!(
        matches!(missing_group, RuscaError::ColumnNotFound { ref column } if column == "ausente"),
        "se esperaba ColumnNotFound, se obtuvo {missing_group:?}"
    );

    let missing_having = database
        .execute("SELECT b, COUNT(*) FROM t GROUP BY b HAVING SUM(ausente) > 1")
        .expect_err("HAVING sobre columna inexistente");
    assert!(
        matches!(missing_having, RuscaError::ColumnNotFound { ref column } if column == "ausente"),
        "se esperaba ColumnNotFound, se obtuvo {missing_having:?}"
    );
}

proptest! {
    /// PBT con oráculo ingenuo: `GROUP BY` multi-clave == agrupación en memoria.
    ///
    /// Genera filas aleatorias sobre un dominio pequeño de etiquetas y compara
    /// los conteos por tupla `(b, c)` contra un `HashMap` ingenuo.
    #[test]
    fn prop_group_by_multi_key_matches_naive_oracle(
        rows in prop::collection::vec(
            (0i64..10, prop::sample::select(vec!["a", "b", "c"]), prop::sample::select(vec!["p", "q"])),
            0..30,
        ),
    ) {
        let (_dir, mut database) = open_test_db("prop0051_oracle");
        create_abc_table(&mut database);
        let triples: Vec<(i64, &str, &str)> = rows
            .iter()
            .map(|(value, second, third)| (*value, *second, *third))
            .collect();
        seed_triples(&mut database, &triples);

        let mut oracle: BTreeMap<(String, String), i64> = BTreeMap::new();
        for (_, second, third) in &rows {
            *oracle
                .entry(((*second).to_string(), (*third).to_string()))
                .or_insert(0) += 1;
        }

        let selected = database
            .execute("SELECT b, c, COUNT(*) FROM t GROUP BY b, c")
            .expect("select");
        prop_assert_eq!(selected.len(), oracle.len());
        for row in &selected {
            let first = match row.get("b") {
                Some(ScalarValue::Text(value)) => value.clone(),
                other => panic!("se esperaba Text en 'b', se obtuvo {other:?}"),
            };
            let second = match row.get("c") {
                Some(ScalarValue::Text(value)) => value.clone(),
                other => panic!("se esperaba Text en 'c', se obtuvo {other:?}"),
            };
            let expected = oracle.get(&(first.clone(), second.clone())).copied().unwrap_or(0);
            prop_assert_eq!(row_count(row), expected);
        }

        // El oráculo con HAVING: solo sobreviven los grupos con conteo > 1.
        let filtered = database
            .execute("SELECT b, c, COUNT(*) FROM t GROUP BY b, c HAVING COUNT(*) > 1")
            .expect("select");
        let survivors = oracle.values().filter(|count| **count > 1).count();
        prop_assert_eq!(filtered.len(), survivors);
    }
}
