//! Tabla virtual en memoria (SPEC-0193, D1): base para `FROM (subquery)` y CTE.
//!
//! Fase 1 del backlog (`docs/RuscaDB-backlog.md` D1/D2): materializa filas
//! `Row` en una tabla sin persistencia; el escaneo futuro es `O(m)`.

use super::*;

/// Tabla virtual materializada (no persistente).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct VirtualTable {
    /// Columnas en orden de primera aparición.
    columns: Vec<String>,
    /// Filas materializadas.
    rows: Vec<Row>,
}

impl VirtualTable {
    /// Materializa filas alineadas a un esquema explícito.
    ///
    /// Args:
    ///     columns: Nombres de columnas del esquema.
    ///     rows: Filas como maps; columnas ausentes quedan `NULL`.
    ///
    /// Returns:
    ///     Tabla virtual lista para escanear.
    #[allow(dead_code)] // helper de tests y de futuras fases
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
        ctes: &std::collections::BTreeMap<String, VirtualTable>,
    ) -> Result<Self, RuscaError> {
        let rows = crate::executor::execute_select_at_ctx(database, select, snapshot, ctes)?;
        Ok(Self::infer_from_rows(rows))
    }

    /// Construye un [`TableDef`] sintético para validar columnas y proyectar.
    ///
    /// Args:
    ///     name: Nombre/alias con el que se referencia la tabla virtual.
    ///
    /// Returns:
    ///     Definición con todas las columnas como `TEXT` (el tipo real se
    ///     conserva en cada [`ScalarValue`]).
    pub(crate) fn table_def(&self, name: &str) -> crate::catalog::TableDef {
        crate::catalog::TableDef {
            name: name.to_string(),
            columns: self
                .columns
                .iter()
                .map(|column| crate::catalog::ColumnDef {
                    name: column.clone(),
                    col_type: crate::catalog::ColumnType::Text,
                })
                .collect(),
            heap_start: ruscadb_storage::PageId(0),
            pages: Vec::new(),
            row_count: self.rows.len() as u64,
            indexes: Vec::new(),
        }
    }

    /// Convierte las filas en registros efímeros para reutilizar el pipeline.
    ///
    /// Returns:
    ///     Un [`Record`] por fila, con metadata neutra (no persistente).
    pub(crate) fn records(&self) -> Vec<ruscadb_core::Record> {
        self.rows
            .iter()
            .map(|row| ruscadb_core::Record {
                id: ruscadb_core::RecordId::new(),
                scalars: row.clone(),
                doc: None,
                edges: ruscadb_core::EdgeSet::default(),
                vector: None,
                blob: None,
                meta: ruscadb_core::RecordMeta::default(),
            })
            .collect()
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
    #[allow(dead_code)] // helper de tests y de futuras fases
    pub(crate) fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Filas materializadas.
    #[allow(dead_code)] // helper de tests y de futuras fases
    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Cardinalidad.
    #[allow(dead_code)] // helper de tests y de futuras fases
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// Indica si no hay filas.
    #[allow(dead_code)] // helper de tests y de futuras fases
    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::{ColumnDef, ColumnType, DbConfig};

    #[test]
    fn virtual_table_materializes_subquery() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("vtable-mat.db");
        let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
        database
            .create_table(
                "t",
                vec![ColumnDef {
                    name: "x".to_string(),
                    col_type: ColumnType::Int,
                }],
            )
            .expect("create");
        for value in 1..=3 {
            database
                .insert(
                    "t",
                    ScalarMap::from([("x".to_string(), ScalarValue::Int(value))]),
                )
                .expect("insert");
        }
        let select = ruscadb_query::parse("SELECT x FROM t WHERE x > 1").expect("parse");
        let snapshot = database.snapshot();
        let table = VirtualTable::materialize_select(
            &mut database,
            &select,
            &snapshot,
            &std::collections::BTreeMap::new(),
        )
        .expect("materializa");
        assert_eq!(table.len(), 2);
        assert_eq!(table.columns(), &["x".to_string()]);
        assert_eq!(table.rows()[0].get("x"), Some(&ScalarValue::Int(2)));
    }

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
