//! Actualización estructurada de una fila viva por `RecordId` (SPEC-0091).
//!
//! Complementa el `UPDATE ... WHERE` textual de RQL (SPEC-0043) con una API de
//! **clave-valor por fila**: `Database::update(table, id, scalars)` localiza la
//! fila por su índice primario (`O(log n)`, SPEC-0024), fusiona los escalares
//! recibidos sobre la versión viva, valida el esquema (con la misma coerción
//! `Int→Float` de `insert`) y persiste la nueva versión MVCC reutilizando la
//! ruta exacta de índices de [`crate::dml`] (índice secundario, HNSW/CSR/
//! invertido).
//!
//! Es la base de las *tools* MCP estructuradas `update`/`delete` (SPEC-0091):
//! el adapter no compone SQL a mano, delega en la fachada y esta aplica RBAC,
//! validación de esquema y auto-commit WAL-first.

use ruscadb_core::{RecordId, RuscaError, ScalarMap};

use crate::catalog::Catalog;
use crate::database::Database;
use crate::table_api::validate_scalars;

impl Database {
    /// Actualiza los escalares indicados de una fila viva por su `RecordId`.
    ///
    /// Fusiona `scalars` sobre la versión viva (las columnas no incluidas
    /// conservan su valor), valida el resultado contra el esquema y persiste la
    /// nueva versión MVCC manteniendo el índice secundario y los índices
    /// derivados. En auto-commit confirma con WAL-first e invalida el caché
    /// semántico; con una transacción activa ([`Database::begin`]) la versión
    /// queda visible al confirmar.
    ///
    /// Es idempotente: si el id no existe o ya está borrado devuelve `Ok(false)`
    /// sin efectos ni pánicos.
    ///
    /// Args:
    ///     table: Tabla destino (debe existir).
    ///     id: Identificador de la fila a actualizar.
    ///     scalars: Columnas a escribir (parciales; el resto se conserva).
    ///
    /// Returns:
    ///     `Ok(true)` si la fila existía viva y quedó actualizada; `Ok(false)`
    ///     si no existe o ya estaba borrada.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::ColumnNotFound`] si una columna no existe;
    ///     [`RuscaError::TypeMismatch`] si un valor no pertenece a su columna;
    ///     [`RuscaError::CorruptManifest`] si la fila actualizada no cabe.
    pub fn update(
        &mut self,
        table: &str,
        id: &RecordId,
        scalars: ScalarMap,
    ) -> Result<bool, RuscaError> {
        self.ensure_writable("update")?;
        // RBAC (SPEC-0066/0070): actualizar datos exige rol Write o superior.
        self.require_write("update")?;
        let mut catalog = Catalog::load(self)?;
        let definition = catalog.get(table)?.clone();
        let Some((locator, record)) = crate::delete::locate_row(self, &definition, id)? else {
            return Ok(false);
        };
        if record.meta.deleted_tx.is_some() {
            return Ok(false);
        }
        let (tx, is_auto_commit) = match self.active_tx {
            Some(tx) => (tx, false),
            None => (self.txn.begin(), true),
        };
        let mut merged = record.scalars.clone();
        merged.extend(scalars);
        let mut updated = record.clone();
        updated.scalars = validate_scalars(&definition, table, merged)?;
        updated.meta.created_tx = tx;
        updated.meta.deleted_tx = None;
        updated.meta.lsn = self.wal.next_lsn();
        crate::dml::persist_row_update(
            self,
            &mut catalog,
            &definition,
            &record,
            &updated,
            locator,
        )?;
        catalog.save(self)?;
        if is_auto_commit {
            self.txn.commit(tx)?;
        }
        self.note_audit("update", table);
        // Coherencia del caché (SPEC-0069): una escritura invalida entradas.
        self.invalidate_semantic_cache();
        Ok(true)
    }
}

/// Construye un [`Record`] con los escalares ya sustituidos (helper de tests).
#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::catalog::{ColumnDef, ColumnType};
    use ruscadb_core::ScalarValue;

    /// Crea una tabla `items(id Int, name Text)` en una base temporal.
    fn seeded_database(tag: &str) -> (std::path::PathBuf, Database, RecordId) {
        let path = std::env::temp_dir().join(format!(
            "ruscadb_row_update_{tag}_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("wal"));
        let mut database = Database::open(crate::DbConfig::new(&path, 32)).expect("open");
        database
            .create_table(
                "items",
                vec![
                    ColumnDef {
                        name: "id".to_string(),
                        col_type: ColumnType::Int,
                    },
                    ColumnDef {
                        name: "name".to_string(),
                        col_type: ColumnType::Text,
                    },
                ],
            )
            .expect("create_table");
        let id = database
            .insert(
                "items",
                ScalarMap::from([
                    ("id".to_string(), ScalarValue::Int(1)),
                    ("name".to_string(), ScalarValue::Text("uno".to_string())),
                ]),
            )
            .expect("insert");
        (path, database, id)
    }

    /// LIMPIEZA: retira los ficheros de la base temporal creada por el test.
    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("wal"));
    }

    /// `update` fusiona columnas parciales y conserva las no indicadas.
    #[test]
    fn update_merges_partial_scalars() {
        let (path, mut database, id) = seeded_database("merge");
        let changed = database
            .update(
                "items",
                &id,
                ScalarMap::from([("name".to_string(), ScalarValue::Text("dos".to_string()))]),
            )
            .expect("update");
        assert!(changed, "la fila viva debe actualizarse");
        let record = database
            .get_record("items", &id)
            .expect("get_record")
            .expect("la fila sigue viva");
        assert_eq!(
            record.scalars.get("id"),
            Some(&ScalarValue::Int(1)),
            "las columnas no indicadas se conservan"
        );
        assert_eq!(
            record.scalars.get("name"),
            Some(&ScalarValue::Text("dos".to_string())),
            "la columna indicada se reescribe"
        );
        cleanup(&path);
    }

    /// `update` devuelve `Ok(false)` para un id inexistente (sin error).
    #[test]
    fn update_missing_id_is_noop() {
        let (path, mut database, _id) = seeded_database("missing");
        let ghost = RecordId::new();
        let changed = database
            .update(
                "items",
                &ghost,
                ScalarMap::from([("name".to_string(), ScalarValue::Text("x".to_string()))]),
            )
            .expect("update de id inexistente no debe fallar");
        assert!(!changed, "un id inexistente no actualiza nada");
        cleanup(&path);
    }

    /// `update` valida el esquema: una columna desconocida es `ColumnNotFound`.
    #[test]
    fn update_rejects_unknown_column() {
        let (path, mut database, id) = seeded_database("unknown_column");
        let error = database
            .update(
                "items",
                &id,
                ScalarMap::from([("nope".to_string(), ScalarValue::Int(9))]),
            )
            .expect_err("una columna desconocida debe fallar");
        assert!(
            matches!(error, RuscaError::ColumnNotFound { .. }),
            "se esperaba ColumnNotFound, se obtuvo {error:?}"
        );
        cleanup(&path);
    }
}
