//! Engine columnar opcional (SPEC-0110): almacenamiento por columnas en memoria.
//!
//! Sin dependencias externas. La integración con Arrow/Lance queda como trabajo
//! futuro (feature `columnar` activa este módulo sin pulls adicionales).

/// Tabla columnar en memoria: un `Vec` por columna.
#[derive(Debug, Clone)]
pub struct ColumnarTable {
    /// Nombres de las columnas en orden de inserción.
    columns: Vec<String>,
    /// Datos por columna (cada entrada es una fila).
    data: Vec<Vec<String>>,
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
    pub fn append(&mut self, row: Vec<String>) -> Result<(), String> {
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
    pub fn scan(&self) -> Vec<Vec<String>> {
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
        table.append(vec!["1".to_string(), "x".to_string()]).expect("append");
        table.append(vec!["2".to_string(), "y".to_string()]).expect("append");
        assert_eq!(table.len(), 2);
        assert_eq!(
            table.scan(),
            vec![
                vec!["1".to_string(), "x".to_string()],
                vec!["2".to_string(), "y".to_string()],
            ]
        );
    }
}
