#![allow(missing_docs)]
//! Engine columnar opcional (SPEC-0110): almacenamiento por columnas en memoria.
//!
//! Sin dependencias externas. La integración con Arrow/Lance queda como trabajo
//! futuro (feature `columnar` activa este módulo sin pulls adicionales).

/// Valor tipado de una celda columnar.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnValue {
    /// Nulo.
    Null,
    /// Booleano.
    Bool(bool),
    /// Entero.
    Int(i64),
    /// Flotante.
    Float(f64),
    /// Texto.
    Text(String),
}

impl ColumnValue {
    /// Convierte a texto.
    pub fn as_text(&self) -> String {
        match self {
            Self::Null => "NULL".to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Int(n) => n.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Text(t) => t.clone(),
        }
    }

    /// Intenta convertir a f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }
}

/// Tabla columnar en memoria: un `Vec` por columna.
#[derive(Debug, Clone)]
pub struct ColumnarTable {
    /// Nombres de las columnas en orden de inserción.
    columns: Vec<String>,
    /// Datos por columna (cada entrada es una fila).
    data: Vec<Vec<ColumnValue>>,
}

impl ColumnarTable {
    /// Crea una tabla columnar vacía con las columnas dadas.
    pub fn new(columns: Vec<String>) -> Self {
        let width = columns.len();
        Self {
            columns,
            data: (0..width).map(|_| Vec::new()).collect(),
        }
    }

    /// Número de columnas.
    pub fn width(&self) -> usize {
        self.columns.len()
    }

    /// Número de filas.
    pub fn len(&self) -> usize {
        self.data.first().map(|c| c.len()).unwrap_or(0)
    }

    /// Indica si la tabla está vacía.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Añade una fila (debe tener tantos valores como columnas).
    pub fn append(&mut self, row: Vec<ColumnValue>) -> Result<(), String> {
        if row.len() != self.columns.len() {
            return Err(format!(
                "fila con {} valores para {} columnas",
                row.len(),
                self.columns.len()
            ));
        }
        for (index, value) in row.into_iter().enumerate() {
            self.data[index].push(value);
        }
        Ok(())
    }

    /// Escanea las filas en orden de inserción.
    pub fn scan(&self) -> Vec<Vec<ColumnValue>> {
        let rows = self.len();
        let mut out = Vec::with_capacity(rows);
        for row_index in 0..rows {
            let mut row = Vec::with_capacity(self.columns.len());
            for column in &self.data {
                row.push(column[row_index].clone());
            }
            out.push(row);
        }
        out
    }

    /// Escanea proyectando solo las columnas indicadas.
    pub fn project(&self, column_names: &[&str]) -> Result<Vec<Vec<ColumnValue>>, String> {
        let indices: Vec<usize> = column_names
            .iter()
            .map(|name| {
                self.columns
                    .iter()
                    .position(|c| c == *name)
                    .ok_or_else(|| format!("columna '{name}' no existe"))
            })
            .collect::<Result<_, _>>()?;
        let rows = self.len();
        let mut out = Vec::with_capacity(rows);
        for row_index in 0..rows {
            let mut row = Vec::with_capacity(indices.len());
            for &idx in &indices {
                row.push(self.data[idx][row_index].clone());
            }
            out.push(row);
        }
        Ok(out)
    }

    /// Filtra filas por igualdad en una columna.
    pub fn filter_eq(
        &self,
        column: &str,
        value: &ColumnValue,
    ) -> Result<Vec<Vec<ColumnValue>>, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        let rows = self.len();
        let mut out = Vec::new();
        for row_index in 0..rows {
            if self.data[idx][row_index] == *value {
                let mut row = Vec::with_capacity(self.columns.len());
                for col in &self.data {
                    row.push(col[row_index].clone());
                }
                out.push(row);
            }
        }
        Ok(out)
    }

    /// Filtra filas por comparación (mayor que) en una columna numérica.
    pub fn filter_gt(&self, column: &str, threshold: f64) -> Result<Vec<Vec<ColumnValue>>, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        let rows = self.len();
        let mut out = Vec::new();
        for row_index in 0..rows {
            if let Some(val) = self.data[idx][row_index].as_f64()
                && val > threshold
            {
                let mut row = Vec::with_capacity(self.columns.len());
                for col in &self.data {
                    row.push(col[row_index].clone());
                }
                out.push(row);
            }
        }
        Ok(out)
    }

    /// Agrega una columna numérica: suma.
    pub fn sum(&self, column: &str) -> Result<f64, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        let mut total = 0.0;
        for val in &self.data[idx] {
            if let Some(v) = val.as_f64() {
                total += v;
            }
        }
        Ok(total)
    }

    /// Agrega una columna numérica: promedio.
    pub fn avg(&self, column: &str) -> Result<f64, String> {
        let s = self.sum(column)?;
        let count = self.len() as f64;
        if count == 0.0 {
            return Ok(0.0);
        }
        Ok(s / count)
    }

    /// Agrega una columna numérica: mínimo.
    pub fn min(&self, column: &str) -> Result<f64, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        self.data[idx]
            .iter()
            .filter_map(|v| v.as_f64())
            .fold(None, |acc, v| match acc {
                None => Some(v),
                Some(a) => Some(a.min(v)),
            })
            .ok_or_else(|| "sin valores numericos".to_string())
    }

    /// Agrega una columna numérica: máximo.
    pub fn max(&self, column: &str) -> Result<f64, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        self.data[idx]
            .iter()
            .filter_map(|v| v.as_f64())
            .fold(None, |acc, v| match acc {
                None => Some(v),
                Some(a) => Some(a.max(v)),
            })
            .ok_or_else(|| "sin valores numericos".to_string())
    }

    /// Nombres de las columnas.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0110-01
    fn test_ac_0110_01_columnar_append_scan() {
        let mut table = ColumnarTable::new(vec!["a".to_string(), "b".to_string()]);
        table
            .append(vec![
                ColumnValue::Int(1),
                ColumnValue::Text("x".to_string()),
            ])
            .expect("append");
        table
            .append(vec![
                ColumnValue::Int(2),
                ColumnValue::Text("y".to_string()),
            ])
            .expect("append");
        assert_eq!(table.len(), 2);
        assert_eq!(table.scan().len(), 2);
        let projected = table.project(&["a"]).expect("project");
        assert_eq!(
            projected,
            vec![vec![ColumnValue::Int(1)], vec![ColumnValue::Int(2)]]
        );
        let filtered = table.filter_eq("a", &ColumnValue::Int(1)).expect("filter");
        assert_eq!(filtered.len(), 1);
        let filtered_gt = table.filter_gt("a", 1.0).expect("filter_gt");
        assert_eq!(filtered_gt.len(), 1);
        assert!((table.sum("a").expect("sum") - 3.0).abs() < 1e-9);
        assert!((table.avg("a").expect("avg") - 1.5).abs() < 1e-9);
        assert!((table.min("a").expect("min") - 1.0).abs() < 1e-9);
        assert!((table.max("a").expect("max") - 2.0).abs() < 1e-9);
    }
}
