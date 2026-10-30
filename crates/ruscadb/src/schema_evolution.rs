//! Evolucion de esquema (SPEC-0160): `ALTER TABLE ADD COLUMN` y gobernanza.
//!
//! El cambio es **metadata-only**: se anade la columna al catalogo persistido y
//! se reconstruyen los indices derivados; las filas existentes no se reescriben
//! y leen `NULL` para la columna nueva (`record.scalars.get` ausente).
//!
//! Las escrituras nuevas siguen el contrato estricto del motor: deben aportar
//! todas las columnas del esquema (la columna nueva incluida, aunque sea
//! `ScalarValue::Null`).

use ruscadb_core::RuscaError;

use crate::catalog::{Catalog, ColumnDef, ColumnType};
use crate::database::Database;

/// Plan de evolucion de una tabla (utilidad de introspeccion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaEvolution {
    table: String,
    columns: Vec<String>,
}

impl SchemaEvolution {
    /// Crea el plan para `table`.
    ///
    /// Args:
    ///     table: Tabla objetivo.
    ///
    /// Returns:
    ///     Plan vacio.
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
        }
    }

    /// Registra una columna a anadir.
    ///
    /// Args:
    ///     column: Nombre de la columna.
    pub fn add_column(&mut self, column: impl Into<String>) {
        self.columns.push(column.into());
    }

    /// Columnas planificadas.
    ///
    /// Returns:
    ///     Nombres en orden de adicion.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Tabla objetivo.
    ///
    /// Returns:
    ///     Nombre de la tabla.
    pub fn table(&self) -> &str {
        &self.table
    }
}

impl Database {
    /// `ALTER TABLE <table> ADD COLUMN <column> <type>` (metadata-only).
    ///
    /// Args:
    ///     table: Tabla existente.
    ///     column: Columna nueva (no debe existir).
    ///     column_type: Tipo de la columna.
    ///
    /// Returns:
    ///     `Ok(())` si la columna se persistio y los indices se reconstruyeron.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::InvalidConfig`] si la columna ya existe o el motor es
    ///     de solo lectura.
    pub fn alter_table_add_column(
        &mut self,
        table: &str,
        column: &str,
        column_type: ColumnType,
    ) -> Result<(), RuscaError> {
        self.ensure_writable("alter_table_add_column")?;
        if column.trim().is_empty() {
            return Err(RuscaError::InvalidConfig(
                "ALTER TABLE: la columna no puede estar vacia".to_string(),
            ));
        }
        let mut catalog = Catalog::load(self)?;
        {
            let definition = catalog.get_mut(table)?;
            if definition
                .columns
                .iter()
                .any(|existing| existing.name == column)
            {
                return Err(RuscaError::InvalidConfig(format!(
                    "ALTER TABLE: la columna '{column}' ya existe en '{table}'"
                )));
            }
            definition.columns.push(ColumnDef {
                name: column.to_string(),
                col_type: column_type,
            });
        }
        catalog.save(self)?;
        self.rebuild_indexes()?;
        Ok(())
    }

    /// Indica si `table` tiene la columna `column`.
    ///
    /// Args:
    ///     table: Tabla.
    ///     column: Columna.
    ///
    /// Returns:
    ///     `true` si existe en el catalogo.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe.
    pub fn has_column(&mut self, table: &str, column: &str) -> Result<bool, RuscaError> {
        let catalog = Catalog::load(self)?;
        Ok(catalog
            .get(table)?
            .columns
            .iter()
            .any(|existing| existing.name == column))
    }

    /// Anade las columnas de gobernanza estandar (idempotente).
    ///
    /// Columnas: `derived_from`, `session_id`, `write_gate_verdict`,
    /// `trust_tier` (todas `TEXT`).
    ///
    /// Args:
    ///     table: Tabla objetivo.
    ///
    /// Returns:
    ///     Nombres de las columnas realmente anadidas (vacio si ya estaban).
    ///
    /// Errors:
    ///     [`RuscaError`] si la tabla no existe o la persistencia falla.
    pub fn add_governance_columns(&mut self, table: &str) -> Result<Vec<String>, RuscaError> {
        let governance = [
            ("derived_from", ColumnType::Text),
            ("session_id", ColumnType::Text),
            ("write_gate_verdict", ColumnType::Text),
            ("trust_tier", ColumnType::Text),
        ];
        let mut added = Vec::new();
        for (name, column_type) in governance {
            if self.has_column(table, name)? {
                continue;
            }
            self.alter_table_add_column(table, name, column_type)?;
            added.push(name.to_string());
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaEvolution;

    /// AC-0160-01: el plan acumula columnas y conserva la tabla.
    #[test]
    // @spec AC-0160-01
    fn test_ac_0160_01_add_column() {
        let mut evolution = SchemaEvolution::new("t");
        evolution.add_column("new_col");
        assert_eq!(evolution.columns(), &["new_col"]);
        assert_eq!(evolution.table(), "t");
    }
}
