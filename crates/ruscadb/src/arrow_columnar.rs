//! Engine columnar Arrow (SPEC-0124): almacenamiento columnar con Apache Arrow.

#[cfg(feature = "arrow-columnar")]
use arrow::array::{ArrayRef, Float64Array, Int64Array, StringArray};
#[cfg(feature = "arrow-columnar")]
use arrow::datatypes::{DataType, Field, Schema};
#[cfg(feature = "arrow-columnar")]
use arrow::record_batch::RecordBatch;
#[cfg(feature = "arrow-columnar")]
use std::sync::Arc;

/// Tipo de columna Arrow.
#[cfg(feature = "arrow-columnar")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrowColumnType {
    /// Entero de 64 bits.
    Int64,
    /// Flotante de 64 bits.
    Float64,
    /// Texto UTF-8.
    Utf8,
}

/// Tabla columnar Arrow en memoria.
#[cfg(feature = "arrow-columnar")]
#[derive(Debug)]
pub struct ArrowColumnarTable {
    schema: Arc<Schema>,
    column_types: Vec<ArrowColumnType>,
    rows: Vec<Vec<String>>,
    batches: Vec<RecordBatch>,
}

#[cfg(feature = "arrow-columnar")]
impl ArrowColumnarTable {
    /// Crea una tabla columnar Arrow con las columnas dadas.
    pub fn new(columns: Vec<(String, ArrowColumnType)>) -> Self {
        let fields: Vec<Field> = columns
            .iter()
            .map(|(name, ct)| {
                let dt = match ct {
                    ArrowColumnType::Int64 => DataType::Int64,
                    ArrowColumnType::Float64 => DataType::Float64,
                    ArrowColumnType::Utf8 => DataType::Utf8,
                };
                Field::new(name, dt, true)
            })
            .collect();
        let schema = Arc::new(Schema::new(fields));
        let column_types = columns.iter().map(|(_, ct)| *ct).collect();
        Self {
            schema,
            column_types,
            rows: Vec::new(),
            batches: Vec::new(),
        }
    }

    /// Número de columnas.
    pub fn num_columns(&self) -> usize {
        self.schema.fields().len()
    }

    /// Número de filas (pendientes + en batches).
    pub fn num_rows(&self) -> usize {
        let batch_rows: usize = self.batches.iter().map(|b| b.num_rows()).sum();
        batch_rows + self.rows.len()
    }

    /// Añade una fila (strings, se convierten según el tipo de columna).
    pub fn append_row(&mut self, values: Vec<String>) {
        self.rows.push(values);
    }

    /// Finaliza las filas pendientes en un RecordBatch.
    pub fn flush(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let mut columns: Vec<ArrayRef> = Vec::new();
        for (col_idx, ct) in self.column_types.iter().enumerate() {
            match ct {
                ArrowColumnType::Int64 => {
                    let vals: Vec<i64> = self
                        .rows
                        .iter()
                        .map(|r| r.get(col_idx).and_then(|s| s.parse().ok()).unwrap_or(0))
                        .collect();
                    columns.push(Arc::new(Int64Array::from(vals)));
                }
                ArrowColumnType::Float64 => {
                    let vals: Vec<f64> = self
                        .rows
                        .iter()
                        .map(|r| r.get(col_idx).and_then(|s| s.parse().ok()).unwrap_or(0.0))
                        .collect();
                    columns.push(Arc::new(Float64Array::from(vals)));
                }
                ArrowColumnType::Utf8 => {
                    let vals: Vec<&str> = self
                        .rows
                        .iter()
                        .map(|r| r.get(col_idx).map(|s| s.as_str()).unwrap_or(""))
                        .collect();
                    columns.push(Arc::new(StringArray::from(vals)));
                }
            }
        }
        if let Ok(batch) = RecordBatch::try_new(self.schema.clone(), columns) {
            self.batches.push(batch);
        }
        self.rows.clear();
    }

    /// Escanea todas las filas como vectores de strings.
    pub fn scan(&self) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        for batch in &self.batches {
            for row in 0..batch.num_rows() {
                let mut r = Vec::new();
                for col in 0..batch.num_columns() {
                    let arr = batch.column(col);
                    if let Some(a) = arr.as_any().downcast_ref::<Int64Array>() {
                        r.push(a.value(row).to_string());
                    } else if let Some(a) = arr.as_any().downcast_ref::<Float64Array>() {
                        r.push(a.value(row).to_string());
                    } else if let Some(a) = arr.as_any().downcast_ref::<StringArray>() {
                        r.push(a.value(row).to_string());
                    } else {
                        r.push("NULL".to_string());
                    }
                }
                out.push(r);
            }
        }
        // Pending rows
        for row in &self.rows {
            out.push(row.clone());
        }
        out
    }

    /// Número de batches.
    pub fn num_batches(&self) -> usize {
        self.batches.len()
    }
}

#[cfg(feature = "arrow-columnar")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0124-01
    fn test_ac_0124_01_arrow_append_scan() {
        let mut table = ArrowColumnarTable::new(vec![
            ("id".to_string(), ArrowColumnType::Int64),
            ("name".to_string(), ArrowColumnType::Utf8),
        ]);
        table.append_row(vec!["1".to_string(), "alice".to_string()]);
        table.append_row(vec!["2".to_string(), "bob".to_string()]);
        assert_eq!(table.num_rows(), 2);
        table.flush();
        assert_eq!(table.num_batches(), 1);
        let scanned = table.scan();
        assert_eq!(scanned.len(), 2);
        assert_eq!(scanned[0], vec!["1", "alice"]);
    }
}
