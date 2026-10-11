//! Time-series cableado en la fachada (SPEC-0057, roadmap §5.2).
//!
//! Cubre AC-0057-01..04: BUCKET con COUNT exacto, WINDOW vs oráculo ingenuo,
//! tabla vacía sin panics y columna no temporal con error accionable. Los
//! agregados se apoyan en `ruscadb-ts` (`Agg`, `SeriesPoint`, `Window`).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use proptest::prelude::*;
use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, RuscaError, ScalarMap, ScalarValue};
use ruscadb_ts::{Agg, SeriesPoint};

/// Abre una base temporal de pruebas.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
    (dir, database)
}

/// Crea `serie(ts INT, v FLOAT)` (ts en milisegundos como entero).
fn create_serie_table(database: &mut Database) {
    database
        .create_table(
            "serie",
            vec![
                ColumnDef {
                    name: "ts".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "v".to_string(),
                    col_type: ColumnType::Float,
                },
            ],
        )
        .expect("create_table");
}

/// Inserta `count` puntos `(ts=i*step, v=valor)` en `serie`.
fn seed_points(database: &mut Database, count: i64, step_ms: i64, value: f64) {
    for index in 0..count {
        let scalars = ScalarMap::from([
            ("ts".to_string(), ScalarValue::Int(index * step_ms)),
            ("v".to_string(), ScalarValue::Float(value)),
        ]);
        database.insert("serie", scalars).expect("insert");
    }
}

/// Oráculo ingenuo O(n·m): suma de valores en `[start, start+window)`.
fn oracle_sum(points: &[SeriesPoint], start: i64, window_ms: i64) -> f64 {
    points
        .iter()
        .filter(|point| point.ts_ms >= start && point.ts_ms < start + window_ms)
        .map(|point| point.value)
        .sum()
}

/// AC-0057-01 — BUCKET agrupa por cubo con conteo exacto.
///
/// Given: 30 puntos con ts creciente (0..29000 paso 1000).
/// When: BUCKET por 10000 ms con COUNT.
/// Then: 3 buckets [0,10000)=10, [10000,20000)=10, [20000,30000)=10.
#[test]
// @spec AC-0057-01
fn test_ac_0057_01_bucket_aggregation() {
    let (_dir, mut database) = open_test_db("ac0057_01");
    create_serie_table(&mut database);
    seed_points(&mut database, 30, 1000, 2.5);

    let buckets = database
        .ts_bucket("serie", "ts", 10_000, None, Agg::Count)
        .expect("bucket");
    assert_eq!(buckets.len(), 3, "tres cubos de 10s: {buckets:?}");
    for (index, bucket) in buckets.iter().enumerate() {
        assert_eq!(bucket.start_ms, index as i64 * 10_000, "inicio alineado");
        assert_eq!(bucket.value, 10.0, "conteo exacto: {bucket:?}");
    }

    // SUM con la métrica `v` constante 2.5 sobre los mismos cubos.
    let sums = database
        .ts_bucket("serie", "ts", 10_000, Some("v"), Agg::Sum)
        .expect("bucket sum");
    assert_eq!(sums.len(), 3, "tres cubos: {sums:?}");
    assert!(
        sums.iter().all(|bucket| (bucket.value - 25.0).abs() < 1e-9),
        "10 puntos * 2.5 = 25 por cubo: {sums:?}"
    );
}

/// AC-0057-02 — WINDOW deslizante coincide con el oráculo ingenuo.
///
/// Given: serie con valores irregulares (ts=0,1000,2000,...; v=ts/1000).
/// When: WINDOW de 3000 ms con paso 1000 y SUM.
/// Then: cada ventana igual a la suma O(n·m) del oráculo.
#[test]
// @spec AC-0057-02
fn test_ac_0057_02_window_slides() {
    let (_dir, mut database) = open_test_db("ac0057_02");
    create_serie_table(&mut database);
    let points: Vec<SeriesPoint> = (0..20)
        .map(|index| SeriesPoint {
            ts_ms: index * 1000,
            value: (index as f64) * 0.5,
        })
        .collect();
    for point in &points {
        let scalars = ScalarMap::from([
            ("ts".to_string(), ScalarValue::Int(point.ts_ms)),
            ("v".to_string(), ScalarValue::Float(point.value)),
        ]);
        database.insert("serie", scalars).expect("insert");
    }

    let windows = database
        .ts_window("serie", "ts", 3000, 1000, Some("v"), Agg::Sum)
        .expect("window");
    assert!(!windows.is_empty(), "hay ventanas para una serie no vacía");
    for window in &windows {
        let expected = oracle_sum(&points, window.start_ms, 3000);
        assert!(
            (window.value - expected).abs() < 1e-9,
            "ventana [{}, {}) = {} vs oráculo {}",
            window.start_ms,
            window.end_ms,
            window.value,
            expected
        );
    }
}

/// AC-0057-03 — tabla vacía devuelve 0 filas sin panics.
///
/// Given: tabla `serie` creada sin filas.
/// When: BUCKET y WINDOW se ejecutan.
/// Then: ambos devuelven 0 filas (sin panic).
#[test]
// @spec AC-0057-03
fn test_ac_0057_03_empty_table() {
    let (_dir, mut database) = open_test_db("ac0057_03");
    create_serie_table(&mut database);

    let buckets = database
        .ts_bucket("serie", "ts", 10_000, None, Agg::Count)
        .expect("bucket vacío");
    assert!(buckets.is_empty(), "sin filas no hay cubos: {buckets:?}");

    let windows = database
        .ts_window("serie", "ts", 3000, 1000, Some("v"), Agg::Sum)
        .expect("window vacía");
    assert!(windows.is_empty(), "sin filas no hay ventanas: {windows:?}");
}

/// AC-0057-04 — columna no temporal o inexistente => error accionable.
///
/// Given: tabla `serie` con columna `v` FLOAT y sin columna `falta`.
/// When: BUCKET se ejecuta sobre `v` (no temporal) y sobre `falta`.
/// Then: `TypeMismatch`/`ColumnNotFound` con mensaje accionable (no panic).
#[test]
// @spec AC-0057-04
fn test_ac_0057_04_non_ts_column_errors() {
    let (_dir, mut database) = open_test_db("ac0057_04");
    create_serie_table(&mut database);
    seed_points(&mut database, 3, 1000, 1.0);

    let error = database
        .ts_bucket("serie", "v", 10_000, None, Agg::Count)
        .expect_err("columna no temporal debe fallar");
    assert!(
        matches!(error, RuscaError::TypeMismatch { .. }),
        "se esperaba TypeMismatch, se obtuvo {error:?}"
    );
    assert!(
        error.to_string().contains("no temporal"),
        "el mensaje debe decir 'no temporal': {error}"
    );

    let missing = database
        .ts_bucket("serie", "falta", 10_000, None, Agg::Count)
        .expect_err("columna inexistente debe fallar");
    assert!(
        matches!(missing, RuscaError::ColumnNotFound { .. }),
        "se esperaba ColumnNotFound, se obtuvo {missing:?}"
    );
}

// PBT — BUCKET COUNT vs oráculo: cada punto cae exactamente en su cubo.
//
// Propiedad: para cualquier serie de `n` puntos con ts creciente, la suma de
// los conteos de todos los cubos == n y cada cubo cubre su intervalo.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    // @spec AC-0057-01
    fn bucket_count_is_partition_of_points(
        count in 0usize..60,
        step in 1i64..5000,
        interval in 1i64..10_000,
    ) {
        let (_dir, mut database) = open_test_db("prop_bucket");
        create_serie_table(&mut database);
        let mut total = 0f64;
        for index in 0..count {
            let scalars = ScalarMap::from([
                ("ts".to_string(), ScalarValue::Int(index as i64 * step)),
                ("v".to_string(), ScalarValue::Float(1.0)),
            ]);
            database.insert("serie", scalars).expect("insert");
            total += 1.0;
        }
        let buckets = database
            .ts_bucket("serie", "ts", interval, None, Agg::Count)
            .expect("bucket");
        let sum: f64 = buckets.iter().map(|bucket| bucket.value).sum();
        prop_assert_eq!(sum, total, "los cubos particionan los puntos");
        for pair in buckets.windows(2) {
            prop_assert!(
                pair[0].end_ms <= pair[1].start_ms,
                "cubos ordenados y sin solape"
            );
        }
    }
}
