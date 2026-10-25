#![allow(missing_docs)]
#![allow(dead_code)]
//! Evolucion de esquema (SPEC-0160): stub.

#[derive(Debug, Clone)]
pub struct SchemaEvolution {
    pub table: String,
    pub columns: Vec<String>,
}

impl SchemaEvolution {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
        }
    }
    pub fn add_column(&mut self, column: impl Into<String>) {
        self.columns.push(column.into());
    }
    pub fn columns(&self) -> &[String] {
        &self.columns
    }
    pub fn table(&self) -> &str {
        &self.table
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0160-01
    fn test_ac_0160_01_add_column() {
        let mut evo = SchemaEvolution::new("t");
        evo.add_column("new_col");
        assert_eq!(evo.columns(), &["new_col"]);
        assert_eq!(evo.table(), "t");
    }
}
