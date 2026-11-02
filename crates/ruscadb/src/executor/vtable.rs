//! Tabla virtual en memoria (SPEC-0193, D1): base para `FROM (subquery)` y CTE.
//!
//! Fase 1 del backlog (`docs/RuscaDB-backlog.md` D1/D2): materializa filas
//! `Row` en una tabla sin persistencia; el escaneo futuro es `O(m)`.

use super::*;

/// Tabla virtual materializada (no persistente).
#[allow(dead_code)] // fase 1: conectores FROM(subquery)/CTE pendientes
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct VirtualTable {
    /// Columnas en orden de primera aparición.
    columns: Vec<String>,
    /// Filas materializadas.
    rows: Vec<Row>,
}

#[allow(dead_code)] // fase 1
impl VirtualTable {
    /// Materializa filas alineadas a un esquema explícito.
    ///
    /// Args:
    ///     columns: Nombres de columnas del esquema.
    ///     rows: Filas como maps; columnas ausentes quedan `NULL`.
    ///
    /// Returns:
    ///     Tabla virtual lista para escanear.
    pub(crate) fn from_rows(columns: Vec<String>, rows: Vec<Row>) -> Self {
        Self { columns, rows }
    }

    /// Ejecuta una subconsulta y la materializa (fase 2 de D1/D2).
    ///
    /// Args:
    ///     database: Base abierta.
    ///     select: Subconsulta ya analizada.
    ///     snapshot: Vista fija de visibilidad MVCC.
    ///
    /// Returns:
    ///     Tabla virtual con la subconsulta materializada una sola vez.
    ///
    /// Errors:
    ///     Los del executor (`execute_select_at`).
    pub(crate) fn materialize_select(
        database: &mut Database,
        select: &ruscadb_query::Select,
        snapshot: &Snapshot,
    ) -> Result<Self, RuscaError> {
        let rows = crate::executor::execute_select_at(database, select, snapshot)?;
        Ok(Self::infer_from_rows(rows))
    }

    /// Materializa infiriendo el esquema de la primera fila.
    ///
    /// Args:
    ///     rows: Filas materializadas (una por subconsulta).
    ///
    /// Returns:
    ///     Tabla virtual; columnas = claves de la primera fila.
    pub(crate) fn infer_from_rows(rows: Vec<Row>) -> Self {
        let columns = rows
            .first()
            .map(|row| row.keys().cloned().collect())
            .unwrap_or_default();
        Self { columns, rows }
    }

    /// Columnas del esquema.
    pub(crate) fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Filas materializadas.
    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Cardinalidad.
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// Indica si no hay filas.
    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Esquema explícito y orden estable.
    #[test]
    fn virtual_table_keeps_columns_and_rows() {
        let rows = vec![
            Row::from([("a".to_string(), ScalarValue::Int(1))]),
            Row::from([("a".to_string(), ScalarValue::Int(2))]),
        ];
        let table = VirtualTable::from_rows(vec!["a".to_string()], rows);
        assert_eq!(table.columns(), &["a".to_string()]);
        assert_eq!(table.len(), 2);
        assert!(!table.is_empty());
        assert_eq!(table.rows()[0].get("a"), Some(&ScalarValue::Int(1)));
    }

    /// Infere el esquema de la primera fila; vacia si no hay filas.
    #[test]
    fn virtual_table_infers_schema() {
        let rows = vec![Row::from([
            ("x".to_string(), ScalarValue::Text("s".to_string())),
            ("y".to_string(), ScalarValue::Null),
        ])];
        let table = VirtualTable::infer_from_rows(rows);
        assert_eq!(table.columns(), &["x".to_string(), "y".to_string()]);
        assert_eq!(table.len(), 1);

        let empty = VirtualTable::infer_from_rows(Vec::new());
        assert!(empty.is_empty());
        assert!(empty.columns().is_empty());
    }
}
