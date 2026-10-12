//! Spike `TableProvider` de DataFusion (SPEC-0060, FR-0060-02/03).
//!
//! Solo se compila con `--features datafusion` (off por defecto). Comprueba
//! la paridad del provider con el scan nativo (AC-0060-02) y el differential
//! relacional del subset WHERE/ORDER BY/proyección (AC-0060-04).
#![cfg(feature = "datafusion")]
#![allow(clippy::expect_used)]

use std::sync::Arc;

use datafusion::arrow::array::{Array, BooleanArray, Float64Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::DataType;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::prelude::SessionContext;
use ruscadb_core::ScalarValue;
use ruscadb_query::provider::RuscaTableProvider;

/// Columnas del spike: `a` (Int), `b` (Float), `t` (Text).
const COLUMNS: [&str; 3] = ["a", "b", "t"];
/// Filas sembradas para la paridad.
const ROWS: i64 = 100;

/// Nombres de columna como `String`.
fn column_names() -> Vec<String> {
    COLUMNS.iter().map(|name| name.to_string()).collect()
}

/// Semilla determinista de `ROWS` filas `(i, i*0.5, "f{i}")`.
fn seed_rows() -> Vec<Vec<ScalarValue>> {
    (0..ROWS)
        .map(|index| {
            vec![
                ScalarValue::Int(index),
                ScalarValue::Float(index as f64 * 0.5),
                ScalarValue::Text(format!("f{index}")),
            ]
        })
        .collect()
}

/// Carga las filas en un `SessionContext` como tabla `spike`.
fn context_with_spike(rows: Vec<Vec<ScalarValue>>) -> SessionContext {
    let context = SessionContext::new();
    let provider = RuscaTableProvider::try_new(column_names(), rows).expect("provider");
    context
        .register_table("spike", Arc::new(provider))
        .expect("register_table");
    context
}

/// Convierte un `RecordBatch` del spike en filas de escalares (por esquema:
/// funciona con cualquier proyección, no solo las 3 columnas).
fn batch_rows(batch: &RecordBatch) -> Vec<Vec<ScalarValue>> {
    (0..batch.num_rows())
        .map(|row| {
            batch
                .columns()
                .iter()
                .map(|column| cell_of(column.as_ref(), row))
                .collect()
        })
        .collect()
}

/// Lee una celda del spike según el tipo Arrow de la columna.
fn cell_of(column: &dyn Array, row: usize) -> ScalarValue {
    if column.is_null(row) {
        return ScalarValue::Null;
    }
    match column.data_type() {
        DataType::Int64 => {
            let values = column.as_any().downcast_ref::<Int64Array>().expect("Int64");
            ScalarValue::Int(values.value(row))
        }
        DataType::Float64 => {
            let values = column
                .as_any()
                .downcast_ref::<Float64Array>()
                .expect("Float64");
            ScalarValue::Float(values.value(row))
        }
        DataType::Utf8 => {
            let values = column.as_any().downcast_ref::<StringArray>().expect("Utf8");
            ScalarValue::Text(values.value(row).to_string())
        }
        DataType::Boolean => {
            let values = column
                .as_any()
                .downcast_ref::<BooleanArray>()
                .expect("Boolean");
            ScalarValue::Bool(values.value(row))
        }
        other => panic!("tipo Arrow no soportado en el spike: {other:?}"),
    }
}

/// Recoge las filas resultantes de una SQL como `Vec<Vec<ScalarValue>>`.
async fn collect_sql(context: &SessionContext, sql: &str) -> Vec<Vec<ScalarValue>> {
    let frame = context.sql(sql).await.expect("sql");
    let batches = frame.collect().await.expect("collect");
    batches.iter().flat_map(batch_rows).collect()
}

/// AC-0060-02 — el scan del provider devuelve exactamente las filas nativas.
///
/// Given: 100 filas sembradas.
/// When: `SELECT * FROM spike` corre en DataFusion sobre nuestro provider.
/// Then: las filas coinciden 1:1 con el scan nativo (paridad total).
#[tokio::test]
// @spec AC-0060-02
async fn test_ac_0060_02_provider_scan_parity() {
    let native = seed_rows();
    let context = context_with_spike(native.clone());
    let scanned = collect_sql(&context, "SELECT * FROM spike").await;
    assert_eq!(scanned, native, "paridad total provider vs scan nativo");
}

/// AC-0060-04 — differential relacional: WHERE + ORDER BY + proyección.
///
/// Given: las mismas 100 filas.
/// When: `SELECT a FROM spike WHERE a > 40 ORDER BY a DESC` corre en
/// DataFusion y en el oráculo nativo en memoria.
/// Then: resultados idénticos en filas y orden.
#[tokio::test]
// @spec AC-0060-04
async fn test_ac_0060_04_differential_relational() {
    let native = seed_rows();
    let context = context_with_spike(native.clone());
    let df_rows = collect_sql(&context, "SELECT a FROM spike WHERE a > 40 ORDER BY a DESC").await;

    // Oráculo nativo: filtro + orden + proyección en memoria.
    let mut oracle: Vec<ScalarValue> = native
        .iter()
        .filter_map(|row| match &row[0] {
            ScalarValue::Int(value) if *value > 40 => Some(row[0].clone()),
            _ => None,
        })
        .collect();
    oracle.sort_by(|left, right| match (left, right) {
        (ScalarValue::Int(l), ScalarValue::Int(r)) => r.cmp(l),
        _ => panic!("el oráculo solo recibe Int en la columna a"),
    });
    let oracle_rows: Vec<Vec<ScalarValue>> = oracle.into_iter().map(|value| vec![value]).collect();

    assert_eq!(
        df_rows, oracle_rows,
        "differential: DataFusion == oráculo nativo (filas y orden)"
    );
}
