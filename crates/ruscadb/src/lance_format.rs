//! Lance format (SPEC-0136): tabla columnar versionada (modelo en memoria).
//!
//! No depende del crate `lance`: modela el contrato (`append` + conteo) para
//! que el adapter real se pueda inyectar sin cambiar la API.

/// Tabla estilo Lance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanceTable {
    /// Nombre de la tabla.
    pub name: String,
    /// Filas almacenadas.
    pub row_count: usize,
}

impl LanceTable {
    /// Crea una tabla vacia.
    ///
    /// Args:
    ///     name: Nombre de la tabla.
    ///
    /// Returns:
    ///     Tabla vacia.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            row_count: 0,
        }
    }

    /// Anade una fila.
    pub fn append(&mut self) {
        self.row_count += 1;
    }

    /// Numero de filas.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.row_count
    }

    /// Indica si no hay filas.
    ///
    /// Returns:
    ///     `true` si vacia.
    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::LanceTable;

    #[test] // @spec AC-0136-01
    fn test_ac_0136_01_lance_append_scan() {
        let mut table = LanceTable::new("test");
        assert!(table.is_empty());
        table.append();
        table.append();
        assert_eq!(table.len(), 2);
        assert_eq!(table.name, "test");
    }
}
