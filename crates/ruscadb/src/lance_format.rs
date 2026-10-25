//! Lance format (SPEC-0136): stub sin dependencias externas.

/// Tabla Lance (stub).
#[derive(Debug, Clone, Default)]
pub struct LanceTable {
    /// Nombre de la tabla.
    pub name: String,
    /// Número de filas.
    pub row_count: usize,
}

impl LanceTable {
    /// Crea una tabla Lance.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            row_count: 0,
        }
    }

    /// Añade una fila (stub).
    pub fn append(&mut self) {
        self.row_count += 1;
    }

    /// Número de filas.
    pub fn len(&self) -> usize {
        self.row_count
    }

    /// Indica si está vacía.
    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0136-01
    fn test_ac_0136_01_lance_append_scan() {
        let mut table = LanceTable::new("test");
        table.append();
        table.append();
        assert_eq!(table.len(), 2);
    }
}
